# Blaze
**Early but usable. Expect breaking changes while the query langauge and CLI settles.**

A *Blaz*ingly fast file searcher.

**blaze** is a fast, index-based file searcher for large codebases and home directories.  It precomputes a compact on-disk index and answers queries in milliseconds, even on trees with millions of paths.

The goal is to provide a syntax-rich searcher that is competitive with `plocate`. `blaze` is designed for free-text-like queries. It is not a sophisticated regex engine and it is not a flag-driven `find` replacement. The query syntax is intentionally small and human-friendly.

## Features

- Very fast substring search
- Not a recursive `find`
- Relevance ranking
- Rich query language
- Low memory overhead

## Usage

`blaze` is designed for free-text-like queries. It is not a sophisticated regex engine and it is not a flag-driven `find` replacement. The syntax is intentionally small and human-friendly.

```bash
blaze query '<query>'
```

### Common searches

Just type words:

```bash
blaze query 'invoice pdf'
blaze query 'src config'
```

Exact phrase (use quotes when there are spaces):

```bash
blaze query '"design doc"'
blaze query '"tax return" 2023'
```

Either / exclude:

```bash
blaze query 'resume or cv'
blaze query 'config not backup'
blaze query '(invoice or receipt) not draft'
```

Simple wildcards:

```bash
blaze query 'Cargo*.toml'
blaze query 'report-202?-final'
```

### Filters

By extension:

```bash
blaze query 'ext:rs'
blaze query 'ext:.jpg vacation'
```

By time (examples):

```bash
blaze query 'modified:today'
blaze query 'modified:-7d'
blaze query 'created:2024-01-01'
blaze query 'modified:this_week ext:md'
```

By size:

```bash
blaze query 'size:>10MB'
blaze query 'size:<500K ext:log'
```

#### Bits and Bytes Smart casing

`size:` defaults to bytes. If you specifically want bits, use an uppercase unit with a lowercase `b` (like `Mb`).

```bash
# bytes (default)
blaze query 'size:>10MB'
blaze query 'size:>10mb'

# bits (uppercase + lowercase b)
blaze query 'size:>10Mb'   # megabits
```

## Performance

Benchmarks were run with [`hyperfine`](https://github.com/sharkdp/hyperfine) on:

- Operating System: PopOS
- Dataset: `$HOME`
- blaze: release build with an index prebuilt over `$HOME` (daemon and CLI)

Each command was run 3 warmup + 10 measured iterations with warm caches.

### `blaze` vs `fdfind` vs `find` vs `plocate`

Representative results (mean time, lower is better):

| Query            | blaze (daemon) | blaze (CLI) | fdfind | plocate | find |
| ---------------- | -------------- | ----------- | ------ | ------- | ---- |
| `ext:rs`         | 1.4 ms         | 2.0 ms      | 11.1 ms| 18.2 ms | 236.9 ms |
| `Cargo.toml`     | 0.7 ms         | 0.9 ms      | 9.1 ms | 2.2 ms  | 223.6 ms |
| `config`         | 1.7 ms         | 2.1 ms      | 10.1 ms| 48.5 ms | 272.6 ms |
| `src`            | 4.8 ms         | 5.0 ms      | 9.7 ms | 141.9 ms| 252.6 ms |
| `modified:today` | 0.7 ms         | 1.2 ms      | 10.3 ms| 1125 ms | 359.7 ms |

Highlights:

- Daemon mode is **2-15x faster** than `fdfind` and **50-540x faster** than `find`, depending on query selectivity.  
- Even against `plocate`, blaze holds a **3-30x** advantage on filename/path queries (and **>1000x** on date filters) while providing a richer query language.  
- Cold-start CLI mode is still **2-10x faster** than `fdfind` and **50-300x faster** than `find`.

## License
MIT
