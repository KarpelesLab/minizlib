# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/KarpelesLab/minizlib/compare/v0.1.0...v0.1.1) - 2026-09-18

### Other

- Add pushed input: Decompressor and BufferedCompressor

## [0.1.0](https://github.com/KarpelesLab/minizlib/releases/tag/v0.1.0) - 2026-09-18

### Other

- Rename the crate to minizlib
- update throughput figure
- Shrink further: infallible bit reads, run table for fixed codes
- Shrink the decoder: register-sized bit reads, one reader for everything
- Require a maximum length wherever nothing bounds the output
- Add CI, footprint guard and release-plz
- Add README, streaming gunzip example, measured footprint
- Add no_std, no-alloc gzip/zlib/deflate decompressor
