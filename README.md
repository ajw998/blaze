# Blaze
**Early but usable. Expect breaking changes while the query langauge and CLI settles.**

A *Blaz*ingly fast file searcher.

**blaze** is a fast, index-based file searcher for large codebases and home directories.  It precomputes a compact on-disk index and answers queries in milliseconds, even on trees with millions of paths.

The goal is to provide a syntax-rich searcher that is competitive with `plocate`.

## Features

- Very fast substring search
- Not a recursive `find`
- Relevance ranking
- Rich query language
- Low memory overhead

## Performance

Benchmarks were run with [`hyperfine`](https://github.com/sharkdp/hyperfine) on:

- Operating System: PopOS
- Dataset: `$HOME`
- blaze: release build with an index prebuilt over `$HOME`

Each command was run 3 warmup + 10 measured iterations with warm caches.

### `blaze` vs `fdfind` vs `find`

Commands:

```bash
# Rareish terms 
blaze query "Cargo.toml"
fdfind Cargo.toml "$HOME"
find "$HOME" -iname "*Cargo.toml*"

# Common term 
blaze query "config"
fdfind config "$HOME"
find "$HOME" -iname "*config*"

# Path substring, expected to return many results 
blaze query "src"
fdfind src "$HOME"
find "$HOME" -iname "*src*"
```

`blaze` is typically **1.7-10x faster** than `fdfind` for common interactive queries, and **~50-230x** faster than `find` once the index has been built.

## Usage

## License
MIT
