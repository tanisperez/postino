# Postino icon

`postino.svg` is the master: the title bar logo (`views/title_bar.rs`), an accent square
(`#4f57d8`, radius 71 of 256) with a "P" in Geist Bold converted to a path, so it renders the
same without the font installed. `crates/postino-app/assets/postino.ico` is the same design.

The PNG sizes under `png/` are generated from it and committed:

```
for s in 16 24 32 48 64 128 256 512; do
  rsvg-convert -w $s -h $s postino.svg -o png/postino-$s.png
done
```

The macOS `.icns` is built from these PNGs at bundle time (`iconutil`, see `packaging/macos`).
