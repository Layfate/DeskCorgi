# The dog's art

`corgi.toml` holds every hand-drawn shape of the dog as SVG path data. The renderer
(`crates/core/src/render.rs`) still draws everything that moves or is generated (legs, paws,
eyes, mouth, tail, keyboard), and skins (`skins/*.toml`) still own the colours. The art file is
compiled into the app, and it is also read straight from disk by the art tool, so **editing it
never needs a recompile**.

## The loop

```sh
cargo run -q -p deskcorgi-core --example art -- check                    # text: parse, symmetry, self-crossings, render sanity
cargo run -q -p deskcorgi-core --example art -- sheet                    # out/art/sheet.png: 4 poses × 3 head turns
cargo run -q -p deskcorgi-core --example art -- sheet --view head --pose idle --turn 0,1 --cell 230
cargo run -q -p deskcorgi-core --example art -- sheet --compare old.toml # every cell as old | new
cargo run -q -p deskcorgi-core --example art -- diff old.toml            # changed pixels per pose, no image
```

Sheet options: `--view dog|head`, `--skin corgi|all`, `--pose idle,happy,focus,sleep`,
`--turn -1,0,1` (any numbers from -1 to 1), `--cell PX` (cell width), `--out FILE`, `--art FILE`.

**Live:** start the app with `DESKCORGI_ART` set to this file's path and every save shows up on
the running dog within half a second (a file that does not parse is ignored and the last good art
stays).

## A part

```toml
[[part]]
id = "head"          # which shape this is (the table below)
symmetric = true     # the front view is its own mirror image; `check` verifies it
note = "..."         # free text for the next person editing
d = "M 0 -49 C ..."  # SVG path data: M L H V C S Q T A Z, upper case absolute, lower case relative
turn = "M 3 -49.5 C ..." # optional 2.5D keyframe, see below
```

## 2.5D turn keyframes

`turn` is the same shape drawn with the head turned to the **viewer's right** as far as it ever
goes (0.42 rad, about 24°). The renderer blends `d` → `turn` by how far the head is turned, and
uses the mirror image for turns to the left, so:

- `d` and `turn` need the same number of segments in each subpath. Every command becomes a cubic
  first, so a straight `L` in one may be a curved `C` in the other.
- A head-space part with a `turn` must be `symmetric` at the front, or it would jump when the head
  crosses the middle.
- Ear parts turn from 0 to 1 only: 1 is the ear on the side the head turns towards, seen edge-on.
  The ear's own +x always points away from the head, so one drawing serves both ears.

Face parts painted on the dome (blaze, muzzle, blush, eyes, nose) already turn in 3D on their
own. Keyframes are for silhouettes, which the dome cannot reshape.

## Spaces

| Space | Origin and axes | Landmarks |
|---|---|---|
| **head** | face centre, x to the viewer's right, y down, before `head_scale` | skull ±62 × ±52 (dome depth 30) · eyes ±24,8 (r ≈ 14) · nose 0,19 · muzzle 0,27 · blush ±40,25 (eye and nose positions are `EYE` / `NOSE` in `render.rs`) · ear bases ±12..±54, -43..-24 |
| **feature** | a small frame stuck to the dome at the feature | nose: tip of the muzzle · brow: ±16,-21 |
| **ear** | inner base -50,0 → outer base 50,0, tip 0,-100 | the pink sits within ±30, -4..-80 |
| **body** | seat 0,0, neck 0,-134, x to the viewer's right | shoulders ≈ ±40,-82 · hips ±64,-28 · chest fur -140..-56 |

## Parts

| id | Space | Drawn as | Colour (from the skin) |
|---|---|---|---|
| `head` | head | silhouette (+ the skin's cheek fluff), clips the face | `body` |
| `face.blaze` / `face.urajiro` | head, on the dome | clipped by the head | `belly` |
| `muzzle` | head, dome + 5 | width × `muzzle_length` | `belly` |
| `blush` | head, dome | viewer-left cheek, mirrored | `blush` |
| `brow` | feature | Shiba dots, only with `brows = true` | `belly` |
| `nose`, `nose.shine` | feature | | `nose`, white |
| `ear.outer`, `ear.inner` | ear | tip width × `ear_pointiness` (drawn at 0.66) | `ear_outer`, `ear_inner` |
| `ear.tuft` | ear | fur tufts, clipped by the ear | `belly` |
| `ear.fur` | ear | thin stroke | `ear_outer` |
| `torso` | body | legs blend into this outline; hips × `butt_fluff` (drawn at 1.25) | `body` → `body_shade` |
| `chest.ruff` / `chest.bib` | body | clipped by the torso | `belly` |

## Style

`[style]` sets line weights and shading without touching code: `head_ink`, `ear_ink`, `tail_ink`,
`head_ink_drop` (heavier line under the head), `head_shade` (lower face in soft shade),
`chin_shadow` (the head's shadow on the chest), `rim_light` (light along the crown),
`crown_shine`, `ear_inner_fade` (pink fading into the fur at the ear's base). `check` prints the
allowed ranges if a value is off.
