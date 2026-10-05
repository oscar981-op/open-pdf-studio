//! Read-only comparison of raw ttf-parser and Hayro's skrifa outline settings.
//! This is an oracle, never used by the DynLex or native-app render path.
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, Target};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::{env, error::Error, fmt::Write, fs};

#[derive(Default)]
struct Recorder(Vec<(char, [f32; 6])>);

impl OutlinePen for Recorder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(('M', [x, y, 0.0, 0.0, 0.0, 0.0]));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(('L', [x, y, 0.0, 0.0, 0.0, 0.0]));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push(('Q', [x1, y1, x, y, 0.0, 0.0]));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(('C', [x1, y1, x2, y2, x, y]));
    }
    fn close(&mut self) {
        self.0.push(('Z', [0.0; 6]));
    }
}

impl ttf_parser::OutlineBuilder for Recorder {
    fn move_to(&mut self, x: f32, y: f32) {
        OutlinePen::move_to(self, x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        OutlinePen::line_to(self, x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        OutlinePen::quad_to(self, x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        OutlinePen::curve_to(self, x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        OutlinePen::close(self);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args.next().ok_or(
        "usage: ttf_outline_oracle FONT.ttf GID ttf|skrifa-unhinted|skrifa-hayro OUTPUT.txt",
    )?;
    let gid: u16 = args
        .next()
        .ok_or("missing GID")?
        .to_string_lossy()
        .parse()?;
    let mode = args.next().ok_or("missing mode")?;
    let output = args.next().ok_or("missing output")?;
    let opq_output = args.next();
    let mut matrix = [0.0_f64; 8];
    if opq_output.is_some() {
        for value in &mut matrix {
            *value = args
                .next()
                .ok_or("missing OPQ1 transform")?
                .to_string_lossy()
                .parse()?;
            if !value.is_finite() || value.abs() > 1_000_000.0 {
                return Err("unbounded OPQ1 transform".into());
            }
        }
    }
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let data = fs::read(input)?;
    if data.len() > 8 * 1024 * 1024 {
        return Err("font exceeds oracle limit".into());
    }
    let mut recorder = Recorder::default();
    let mut interpreter_used = false;
    let mut scale = 1.0_f64;
    match mode.to_str().ok_or("invalid mode")? {
        "ttf" => {
            let face = ttf_parser::Face::parse(&data, 0)?;
            face.outline_glyph(ttf_parser::GlyphId(gid), &mut recorder)
                .ok_or("glyph has no outline")?;
        }
        "skrifa-unhinted" | "skrifa-hayro" => {
            let font = FontRef::from_index(&data, 0)?;
            // TGR1's basis uses the original UPEM; Hayro asks skrifa to draw
            // at 1000 units. Restore the original basis without discarding
            // skrifa's f32/fixed-point outline coordinates.
            scale = f64::from(ttf_parser::Face::parse(&data, 0)?.units_per_em()) / 1000.0;
            let outlines = font.outline_glyphs();
            let glyph = outlines
                .get(GlyphId::new(u32::from(gid)))
                .ok_or("glyph has no outline")?;
            let instance = if mode == "skrifa-hayro" && outlines.require_interpreter() {
                HintingInstance::new(
                    &outlines,
                    Size::new(1000.0),
                    LocationRef::default(),
                    HintingOptions {
                        engine: Engine::Interpreter,
                        target: Target::Mono,
                    },
                )
                .ok()
            } else {
                None
            };
            let settings = if let Some(instance) = instance.as_ref() {
                interpreter_used = true;
                DrawSettings::hinted(instance, false)
            } else {
                DrawSettings::unhinted(Size::new(1000.0), LocationRef::default())
            };
            glyph.draw(settings, &mut recorder)?;
        }
        _ => return Err("unknown outline mode".into()),
    }
    if recorder.0.is_empty() || recorder.0.len() > 1024 {
        return Err("empty or unbounded glyph outline".into());
    }
    if let Some(opq_output) = opq_output {
        if recorder.0.len() > 272 {
            return Err("OPQ1 record limit exceeded".into());
        }
        let contour_count = recorder.0.iter().filter(|(op, _)| *op == 'M').count();
        let mut bytes = Vec::with_capacity(20 + 40 * recorder.0.len());
        bytes.extend_from_slice(b"OPQ1");
        for word in [1_u32, contour_count as u32, recorder.0.len() as u32, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let map = |x: f32, y: f32| -> Result<[f64; 2], Box<dyn Error>> {
            let x = f64::from(x) * scale;
            let y = f64::from(y) * scale;
            let p = [
                matrix[0] * x + matrix[2] * y + matrix[4] + matrix[6],
                matrix[1] * x + matrix[3] * y + matrix[5] + matrix[7],
            ];
            if p.iter()
                .any(|v| !v.is_finite() || !(0.0..=4096.0).contains(v))
            {
                return Err("mapped outline point outside OPQ1 limit".into());
            }
            Ok(p)
        };
        for (op, values) in &recorder.0 {
            let (opcode, payload) = match op {
                'M' => {
                    let p = map(values[0], values[1])?;
                    (1_u32, [p[0], p[1], 0.0, 0.0])
                }
                'L' => {
                    let p = map(values[0], values[1])?;
                    (2, [p[0], p[1], 0.0, 0.0])
                }
                'Q' => {
                    let c = map(values[0], values[1])?;
                    let p = map(values[2], values[3])?;
                    (3, [c[0], c[1], p[0], p[1]])
                }
                'Z' => (4, [0.0; 4]),
                _ => return Err("cubic outline cannot be written as OPQ1".into()),
            };
            bytes.extend_from_slice(&opcode.to_le_bytes());
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            for value in payload {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        fs::write(opq_output, bytes)?;
    }
    let mut text = format!(
        "# gid={gid} mode={} interpreter_used={interpreter_used}\n",
        mode.to_string_lossy()
    );
    for (opcode, values) in recorder.0 {
        write!(text, "{opcode}")?;
        let len = match opcode {
            'M' | 'L' => 2,
            'Q' => 4,
            'C' => 6,
            _ => 0,
        };
        for value in &values[..len] {
            write!(text, " {value:.9}")?;
        }
        writeln!(text)?;
    }
    fs::write(output, text)?;
    Ok(())
}
