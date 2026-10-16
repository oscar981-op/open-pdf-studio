//! Conservative structural/decode preflight for the text research frontend.
//! This is NOT a complete PDF validator or a replacement for parser isolation.
//! It budgets lexical nesting and object-stream expansion before lopdf loads.
use std::collections::BTreeMap;
const SOURCE: usize = 16_000_000;
const EXPANSION: usize = 4 * 1024 * 1024;
const TOTAL_EXPANSION: usize = 32 * 1024 * 1024;
const TOKENS: usize = 262_144;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Debug, Default)]
pub struct Stats {
    pub tokens: usize,
    pub streams: usize,
    pub object_streams: usize,
    pub decoded_bytes: usize,
}
#[derive(Debug)]
enum Value {
    Int(usize),
    Ref(usize, usize),
    Name(Vec<u8>),
    Array(Vec<Value>),
    Dict(BTreeMap<Vec<u8>, Value>),
    Other,
}
struct XrefLengths {
    offsets: BTreeMap<(usize, usize), usize>,
}
impl XrefLengths {
    fn read_number(bytes: &[u8], at: &mut usize, limit: usize) -> Result<usize> {
        while bytes.get(*at).is_some_and(|b| white(*b)) {
            *at += 1;
        }
        let start = *at;
        while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
        if *at == start || !bytes.get(*at).is_some_and(|b| white(*b)) {
            return Err("xref integer syntax");
        }
        let value = std::str::from_utf8(&bytes[start..*at])
            .map_err(|_| "xref integer")?
            .parse::<usize>()
            .map_err(|_| "xref integer overflow")?;
        if value > limit {
            return Err("xref integer budget");
        }
        Ok(value)
    }
    fn at(bytes: &[u8], at: &mut usize, word: &[u8]) -> Result<()> {
        while bytes.get(*at).is_some_and(|b| white(*b)) {
            *at += 1;
        }
        if bytes.get(*at..*at + word.len()) != Some(word)
            || !bytes.get(*at + word.len()).is_some_and(|b| white(*b))
        {
            return Err("xref keyword syntax");
        }
        *at += word.len();
        Ok(())
    }
    fn parse(bytes: &[u8]) -> Result<Self> {
        let start = bytes
            .windows(b"startxref".len())
            .rposition(|w| w == b"startxref")
            .ok_or("indirect Length requires classic xref")?;
        let mut at = start + b"startxref".len();
        let offset = Self::read_number(bytes, &mut at, bytes.len())?;
        let mut at = offset;
        Self::at(bytes, &mut at, b"xref")?;
        let mut offsets = BTreeMap::new();
        let mut entries = 0_usize;
        loop {
            while bytes.get(at).is_some_and(|b| white(*b)) {
                at += 1;
            }
            if bytes.get(at..at + 7) == Some(b"trailer") {
                break;
            }
            let first = Self::read_number(bytes, &mut at, 65_536)?;
            let count = Self::read_number(bytes, &mut at, 65_536)?;
            if first.checked_add(count).is_none_or(|n| n > 65_536) {
                return Err("xref section range");
            }
            entries = entries.checked_add(count).ok_or("xref entry overflow")?;
            if entries > 65_536 {
                return Err("xref entry budget");
            }
            for number in first..first + count {
                let offset = Self::read_number(bytes, &mut at, bytes.len())?;
                let generation = Self::read_number(bytes, &mut at, 65_535)?;
                while bytes.get(at).is_some_and(|b| white(*b)) {
                    at += 1;
                }
                let status = *bytes.get(at).ok_or("truncated xref entry")?;
                at += 1;
                if !bytes.get(at).is_some_and(|b| white(*b)) {
                    return Err("xref entry status delimiter");
                }
                match status {
                    b'n' => {
                        if offsets.insert((number, generation), offset).is_some() {
                            return Err("duplicate xref entry");
                        }
                    }
                    b'f' => {}
                    _ => return Err("xref entry status"),
                }
            }
        }
        if entries == 0 {
            return Err("empty xref");
        }
        Ok(Self { offsets })
    }
    fn length(&self, bytes: &[u8], object: usize, generation: usize) -> Result<usize> {
        let mut at = *self
            .offsets
            .get(&(object, generation))
            .ok_or("indirect Length is absent from xref")?;
        if Self::read_number(bytes, &mut at, 65_535)? != object
            || Self::read_number(bytes, &mut at, 65_535)? != generation
        {
            return Err("indirect Length xref target mismatch");
        }
        Self::at(bytes, &mut at, b"obj")?;
        let length = Self::read_number(bytes, &mut at, bytes.len())?;
        Self::at(bytes, &mut at, b"endobj")?;
        Ok(length)
    }
}
fn white(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}
fn delim(b: u8) -> bool {
    white(b) || b"()<>[]{}/%".contains(&b)
}
struct Scan<'a, 'b> {
    bytes: &'a [u8],
    at: usize,
    stats: &'b mut Stats,
    xref: &'b mut Option<XrefLengths>,
}
impl Scan<'_, '_> {
    fn skip(&mut self) {
        loop {
            while self.bytes.get(self.at).is_some_and(|b| white(*b)) {
                self.at += 1;
            }
            if self.bytes.get(self.at) != Some(&b'%') {
                break;
            }
            while self
                .bytes
                .get(self.at)
                .is_some_and(|b| !matches!(b, 10 | 13))
            {
                self.at += 1;
            }
        }
    }
    fn token(&mut self) -> Result<()> {
        self.stats.tokens += 1;
        if self.stats.tokens > TOKENS {
            return Err("preflight token budget");
        }
        Ok(())
    }
    fn name(&mut self) -> Result<Vec<u8>> {
        if self.bytes.get(self.at) != Some(&b'/') {
            return Err("dictionary key is not a name");
        }
        self.at += 1;
        let mut name = Vec::new();
        while self.bytes.get(self.at).is_some_and(|b| !delim(*b)) {
            let b = self.bytes[self.at];
            self.at += 1;
            if b == b'#' {
                let hex = self
                    .bytes
                    .get(self.at..self.at + 2)
                    .ok_or("short escaped name")?;
                let h = std::str::from_utf8(hex).map_err(|_| "bad name escape")?;
                name.push(u8::from_str_radix(h, 16).map_err(|_| "bad name escape")?);
                self.at += 2;
            } else {
                name.push(b);
            }
            if name.len() > 128 {
                return Err("name budget");
            }
        }
        Ok(name)
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        if depth > 32 {
            return Err("preflight nesting budget");
        }
        self.skip();
        self.token()?;
        let b = *self.bytes.get(self.at).ok_or("truncated value")?;
        match b {
            b'/' => Ok(Value::Name(self.name()?)),
            b'[' => {
                self.at += 1;
                let mut array = Vec::new();
                loop {
                    self.skip();
                    if self.bytes.get(self.at) == Some(&b']') {
                        self.at += 1;
                        break;
                    }
                    if array.len() >= 8192 {
                        return Err("array budget");
                    }
                    array.push(self.value(depth + 1)?);
                }
                Ok(Value::Array(array))
            }
            b'<' if self.bytes.get(self.at + 1) == Some(&b'<') => {
                self.at += 2;
                let mut dict = BTreeMap::new();
                loop {
                    self.skip();
                    if self.bytes.get(self.at..self.at + 2) == Some(b">>") {
                        self.at += 2;
                        break;
                    }
                    if dict.len() >= 256 {
                        return Err("dictionary budget");
                    }
                    let key = self.name()?;
                    let val = self.value(depth + 1)?;
                    // Consume an indirect reference as one dictionary value.
                    let checkpoint = self.at;
                    self.skip();
                    let start = self.at;
                    while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                        self.at += 1;
                    }
                    if let (Value::Int(object), true) = (&val, self.at > start) {
                        let generation = std::str::from_utf8(&self.bytes[start..self.at])
                            .map_err(|_| "reference generation")?
                            .parse::<usize>()
                            .map_err(|_| "reference generation overflow")?;
                        if generation > 65_535 {
                            return Err("reference generation budget");
                        }
                        self.skip();
                        if self.bytes.get(self.at) == Some(&b'R')
                            && self.bytes.get(self.at + 1).is_none_or(|b| delim(*b))
                        {
                            self.at += 1;
                            if dict.insert(key, Value::Ref(*object, generation)).is_some() {
                                return Err("duplicate dictionary key");
                            }
                            continue;
                        }
                    }
                    self.at = checkpoint;
                    if dict.insert(key, val).is_some() {
                        return Err("duplicate dictionary key");
                    }
                }
                Ok(Value::Dict(dict))
            }
            b'(' => {
                self.at += 1;
                let start = self.at;
                let mut nested = 1;
                while nested > 0 {
                    let b = *self.bytes.get(self.at).ok_or("unclosed string")?;
                    self.at += 1;
                    match b {
                        b'\\' => {
                            if self.at >= self.bytes.len() {
                                return Err("string escape");
                            }
                            self.at += 1;
                        }
                        b'(' => nested += 1,
                        b')' => nested -= 1,
                        _ => {}
                    }
                    if nested > 32 || self.at - start > 65536 {
                        return Err("string budget");
                    }
                }
                Ok(Value::Other)
            }
            b'<' => {
                self.at += 1;
                let start = self.at;
                loop {
                    let b = *self.bytes.get(self.at).ok_or("unclosed hex string")?;
                    self.at += 1;
                    if b == b'>' {
                        break;
                    }
                    if !white(b) && !b.is_ascii_hexdigit() {
                        return Err("hex string syntax");
                    }
                    if self.at - start > 131072 {
                        return Err("hex string budget");
                    }
                }
                Ok(Value::Other)
            }
            b']' | b'>' | b')' | b'{' | b'}' => Err("unexpected delimiter"),
            _ => {
                let start = self.at;
                while self.bytes.get(self.at).is_some_and(|b| !delim(*b)) {
                    self.at += 1;
                }
                if self.at == start || self.at - start > 64 {
                    return Err("atom budget");
                }
                let word = &self.bytes[start..self.at];
                if word.iter().all(u8::is_ascii_digit) {
                    Ok(Value::Int(
                        std::str::from_utf8(word)
                            .map_err(|_| "integer")?
                            .parse()
                            .map_err(|_| "integer overflow")?,
                    ))
                } else {
                    Ok(Value::Other)
                }
            }
        }
    }
    fn all(&mut self, allow_streams: bool) -> Result<()> {
        let mut trailer_pending = false;
        while {
            self.skip();
            self.at < self.bytes.len()
        } {
            let start = self.at;
            let value = self.value(0)?;
            let trailer = self.bytes.get(start..self.at) == Some(b"trailer");
            if trailer_pending {
                let Value::Dict(dict) = &value else {
                    return Err("trailer dictionary required");
                };
                if !matches!(dict.get(b"Size".as_slice()),Some(Value::Int(n)) if *n<=65536) {
                    return Err("xref Size budget");
                }
            }
            trailer_pending = trailer;
            self.skip();
            let Some(tail) = self.bytes.get(self.at..) else {
                return Err("offset");
            };
            if tail.starts_with(b"stream") && tail.get(6).is_some_and(|b| white(*b)) {
                if !allow_streams {
                    return Err("stream inside object stream");
                }
                let Value::Dict(dict) = value else {
                    return Err("stream without dictionary");
                };
                let length = match dict.get(b"Length".as_slice()) {
                    Some(Value::Int(length)) => *length,
                    Some(Value::Ref(object, generation)) if allow_streams => {
                        if self.xref.is_none() {
                            *self.xref = Some(XrefLengths::parse(self.bytes)?);
                        }
                        self.xref.as_ref().ok_or("missing xref")?.length(
                            self.bytes,
                            *object,
                            *generation,
                        )?
                    }
                    _ => return Err("stream Length must be direct or verified xref reference"),
                };
                self.at += 6;
                if self.bytes.get(self.at) == Some(&b'\r') {
                    self.at += 1;
                }
                if self.bytes.get(self.at) == Some(&b'\n') {
                    self.at += 1;
                } else if self.bytes.get(self.at - 1) != Some(&b'\r') {
                    return Err("stream EOL");
                }
                let data = self
                    .bytes
                    .get(self.at..self.at.checked_add(length).ok_or("stream overflow")?)
                    .ok_or("truncated stream")?;
                self.at += length;
                self.skip();
                if self.bytes.get(self.at..self.at + 9) != Some(b"endstream") {
                    return Err("stream length disagrees with endstream");
                }
                self.at += 9;
                self.stats.streams += 1;
                if self.stats.streams > 4096 {
                    return Err("stream count budget");
                }
                let kind = match dict.get(b"Type".as_slice()) {
                    Some(Value::Name(n)) => n.as_slice(),
                    None => b"",
                    _ => return Err("indirect stream type unsupported"),
                };
                if matches!(kind, b"ObjStm" | b"XRef") {
                    if kind == b"ObjStm" {
                        if !matches!(dict.get(b"N".as_slice()),Some(Value::Int(n)) if *n<=4096)
                            || !matches!(dict.get(b"First".as_slice()),Some(Value::Int(n)) if *n<=EXPANSION)
                        {
                            return Err("object stream header budget");
                        }
                        self.stats.object_streams += 1;
                    }
                    if kind == b"XRef" {
                        if !matches!(dict.get(b"Size".as_slice()),Some(Value::Int(n)) if *n<=65536)
                        {
                            return Err("xref entry budget");
                        }
                        if !matches!(dict.get(b"W".as_slice()),Some(Value::Array(a)) if a.len()==3 && a.iter().all(|v| matches!(v,Value::Int(n) if *n<=8)) && a.iter().any(|v|matches!(v,Value::Int(n) if *n>0)))
                        {
                            return Err("xref field widths");
                        }
                        if let Some(index) = dict.get(b"Index".as_slice()) {
                            let Value::Array(index) = index else {
                                return Err("xref Index array");
                            };
                            if index.len() % 2 != 0 {
                                return Err("xref Index pairs");
                            }
                            let mut entries = 0_usize;
                            for pair in index.chunks_exact(2) {
                                let (Value::Int(first), Value::Int(count)) = (&pair[0], &pair[1])
                                else {
                                    return Err("xref Index integers");
                                };
                                if first.checked_add(*count).is_none_or(|n| n > 65536) {
                                    return Err("xref Index range");
                                }
                                entries =
                                    entries.checked_add(*count).ok_or("xref Index overflow")?;
                                if entries > 65536 {
                                    return Err("xref Index budget");
                                }
                            }
                        }
                        if let Some(params) = dict.get(b"DecodeParms".as_slice()) {
                            let Value::Dict(params) = params else {
                                return Err("xref predictor dictionary required");
                            };
                            for (key, value) in params {
                                let valid = match key.as_slice() {
                                    b"Predictor" => matches!(
                                        value,
                                        Value::Int(1 | 2 | 10 | 11 | 12 | 13 | 14 | 15)
                                    ),
                                    b"Columns" => {
                                        matches!(value,Value::Int(n) if (1..=64).contains(n))
                                    }
                                    b"Colors" => matches!(value, Value::Int(1)),
                                    b"BitsPerComponent" => matches!(value, Value::Int(8)),
                                    _ => false,
                                };
                                if !valid {
                                    return Err("xref predictor budget");
                                }
                            }
                        }
                    }
                    let remaining = TOTAL_EXPANSION - self.stats.decoded_bytes;
                    let limit = EXPANSION.min(remaining);
                    let flate = match dict.get(b"Filter".as_slice()) {
                        Some(Value::Name(n)) => n == b"FlateDecode",
                        Some(Value::Array(a)) if a.len() == 1 => {
                            matches!(&a[0],Value::Name(n) if n==b"FlateDecode")
                        }
                        _ => false,
                    };
                    let decoded = if flate {
                        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, limit)
                            .map_err(|_| "object/xref stream decode budget or invalid Flate")?
                    } else if !dict.contains_key(b"Filter".as_slice()) && data.len() <= limit {
                        data.to_vec()
                    } else {
                        return Err("unsupported object/xref stream filter");
                    };
                    self.stats.decoded_bytes = self
                        .stats
                        .decoded_bytes
                        .checked_add(decoded.len())
                        .ok_or("decode overflow")?;
                    if self.stats.decoded_bytes > TOTAL_EXPANSION {
                        return Err("aggregate object/xref expansion budget");
                    }
                    if kind == b"ObjStm" {
                        if dict.contains_key(b"DecodeParms".as_slice()) {
                            return Err("object stream DecodeParms unsupported");
                        }
                        let mut no_xref = None;
                        Scan {
                            bytes: &decoded,
                            at: 0,
                            stats: self.stats,
                            xref: &mut no_xref,
                        }
                        .all(false)?;
                    }
                }
            }
        }
        if trailer_pending {
            return Err("trailer dictionary required");
        }
        Ok(())
    }
}
pub fn inspect(bytes: &[u8]) -> Result<Stats> {
    if bytes.len() > SOURCE || !bytes.starts_with(b"%PDF-") {
        return Err("PDF source budget/header");
    }
    let mut stats = Stats::default();
    let mut xref = None;
    Scan {
        bytes,
        at: 0,
        stats: &mut stats,
        xref: &mut xref,
    }
    .all(true)?;
    Ok(stats)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn stream(body: &[u8], extra: &str) -> Vec<u8> {
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(body, 6);
        [format!("%PDF-1.7\n1 0 obj << /Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /Length {} {extra} >>\nstream\n",packed.len()).into_bytes(),packed,b"\nendstream\nendobj".to_vec()].concat()
    }
    #[test]
    fn object_stream_nesting_and_decode_limits() {
        assert_eq!(
            inspect(&stream(b"2 0 << /A [1 2 3] >>", ""))
                .unwrap()
                .object_streams,
            1
        );
        assert!(inspect(&stream(&vec![b' '; EXPANSION + 1], "")).is_err());
        assert!(inspect(&stream(
            format!("{}0{}", "[".repeat(33), "]".repeat(33)).as_bytes(),
            ""
        ))
        .is_err());
        assert!(inspect(&stream(b"2 0 << /A 1 >>", "/DecodeParms <<>>")).is_err());
    }
    #[test]
    fn aggregate_expansion_and_xref_headers() {
        // Each stream is below its individual limit; the ninth exceeds the
        // remaining shared budget before decompression allocates its output.
        let part = stream(&vec![b' '; EXPANSION], "");
        let mut pdf = part.repeat(9);
        assert!(inspect(&pdf).is_err());
        pdf = part.repeat(8);
        assert_eq!(inspect(&pdf).unwrap().decoded_bytes, TOTAL_EXPANSION);
        let good=b"%PDF-1.7\n<< /Type /XRef /Size 8 /W [1 2 1] /Index [0 8] /Length 0 >>stream\n\nendstream";
        assert!(inspect(good).is_ok());
        let text = String::from_utf8(good.to_vec()).unwrap();
        for changed in [
            text.replace("[0 8]", "[0 999999999]"),
            text.replace("[1 2 1]", "[1 99 1]"),
            text.replace(
                "/Length 0",
                "/DecodeParms << /Columns 99999999 >> /Length 0",
            ),
        ] {
            assert!(inspect(changed.as_bytes()).is_err());
        }
    }

    #[test]
    fn framing_and_dictionary_validation() {
        for b in [
            b"%PDF-1.7\n<< /Length 5 >>stream\nabc\nendstream".as_slice(),
            b"%PDF-1.7\ntrailer << /Size 999999999 >>",
            b"%PDF-1.7\n<< /A 1 /A 2 >>",
            b"%PDF-1.7\n(unclosed",
            b"%PDF-1.7\n<< /Length 1 0 R >>stream\na\nendstream",
        ] {
            assert!(inspect(b).is_err());
        }
        // /Size is also a legitimate array in a sampled PDF function. Only
        // xref streams and actual trailers have an xref entry budget.
        assert!(inspect(b"%PDF-1.7\n1 0 obj << /FunctionType 0 /Size [255] >> endobj").is_ok());
        assert!(inspect(b"%PDF-1.7\ntrailer << /Size 42 >>").is_ok());
        assert!(inspect(b"%PDF-1.7\ntrailer 42").is_err());
        let bytes = stream(b"2 0 << /Name (stream\\(ok\\)) /Ref 3 0 R >>", "");
        assert!(inspect(&bytes).is_ok());
        for i in 0..bytes.len() {
            let mut b = bytes.clone();
            b[i] ^= 255;
            assert!(std::panic::catch_unwind(|| inspect(&b)).is_ok());
        }
    }
    #[test]
    fn classic_xref_resolves_bounded_indirect_stream_length() {
        let mut pdf = b"%PDF-1.7\n".to_vec();
        let stream_offset = pdf.len();
        pdf.extend(b"1 0 obj << /Length 3 0 R >>\nstream\nabc\nendstream\nendobj\n");
        let length_offset = pdf.len();
        pdf.extend(b"3 0 obj 3 endobj\n");
        let xref_offset = pdf.len();
        pdf.extend(format!("xref\n0 4\n0000000000 65535 f \n{stream_offset:010} 00000 n \n0000000000 00000 f \n{length_offset:010} 00000 n \ntrailer\n<< /Size 4 >>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
        assert_eq!(inspect(&pdf).unwrap().streams, 1);
        for bad in [
            b"4 0 obj 3 endobj".as_slice(),
            b"3 0 obj 2 endobj",
            b"3 0 obj 9 endobj",
        ] {
            let mut changed = pdf.clone();
            changed[length_offset..length_offset + bad.len()].copy_from_slice(bad);
            assert!(inspect(&changed).is_err());
        }
        let mut wrong_offset = pdf;
        let entry = format!("{length_offset:010} 00000 n");
        let where_entry = wrong_offset
            .windows(entry.len())
            .position(|w| w == entry.as_bytes())
            .unwrap();
        wrong_offset[where_entry..where_entry + 10].copy_from_slice(b"0000000000");
        assert!(inspect(&wrong_offset).is_err());
    }
}
