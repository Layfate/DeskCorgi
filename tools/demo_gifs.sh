#!/usr/bin/env sh
# Rebuilds the README demo GIFs (assets/demo_typing.gif, demo_mouse.gif, demo_pet.gif).
# Needs ffmpeg on the PATH. Run from the repository root: sh tools/demo_gifs.sh
set -eu
cargo run -q --release -p deskcorgi-core --example demo_gifs
for name in typing mouse pet; do
  ffmpeg -v error -y -framerate 25 -i "out/demo/$name/f%04d.png" \
    -vf "split[a][b];[a]palettegen=max_colors=256:stats_mode=full[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
    -loop 0 "assets/demo_$name.gif"
  echo "assets/demo_$name.gif: $(wc -c < "assets/demo_$name.gif") bytes"
done
