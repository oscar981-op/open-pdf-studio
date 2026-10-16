#!/usr/bin/env bash
# Build the native app and its trusted sibling worker together.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p opds-dynlex-backend --features kernel --bin opds-text-worker
cargo build --release -p opds-native-app --all-features
