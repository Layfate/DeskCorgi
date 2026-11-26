---
name: corgi-art
description: Draw, redraw or polish DeskCorgi's dog art (head, face markings, muzzle, ears, nose, torso, chest fur, 2.5D head-turn keyframes, line weights and shading) by editing art/corgi.toml and checking it with the no-recompile art tool. Use for any change to how the corgi looks, a new 2D or 2.5D detail, or a "make the corgi cuter/better" request.
---

# Corgi art

The dog's drawn shapes live in `art/corgi.toml` as SVG path data. Read `art/README.md` first:
it has the spaces, landmarks, part ids and keyframe rules. Edit the art file, not `render.rs`,
unless the change needs a new part or a new kind of drawing (then add the part to
`art::REQUIRED`, draw it in `render.rs`, and document it in the README table).

## Loop (cheap on tokens)

1. Snapshot first: copy `art/corgi.toml` to the scratchpad as `before.toml`.
2. Edit the paths or `[style]` numbers.
3. `cargo run -q -p deskcorgi-core --example art -- check`. Fix every ISSUE before looking at
   any picture. It costs no image tokens.
4. Look at **one small, targeted** sheet:
   - a face detail: `sheet --view head --pose idle --turn 0,1 --cell 230 --compare before.toml`
   - a 2.5D turn: `sheet --view head --pose idle --turn -1,-0.5,0,0.5,1 --cell 140`
   - the whole dog: `sheet --pose idle,happy --cell 160`
   - before finishing: `sheet --view head --pose idle,happy --turn 0,1 --cell 150`
5. When a change should be invisible (a refactor, tidying a path), use `diff before.toml`
   instead of a picture.
6. Repeat 2–4. Then run `cargo test -p deskcorgi-core`.

Rules: one Read of `out/art/sheet.png` per iteration. Never read a full-resolution render to
judge a shape. Use `--compare` whenever you are judging whether a change helped.

## Drawing rules

- Head space is about 124 × 104 units at the front. A head unit is roughly 1–2 px on screen,
  depending on the size setting, so changes under about 1.5 units don't show.
- Keep curves made of few, long cubics. Five segments draw the whole head outline. Many tiny
  segments make wobbly outlines.
- Silhouettes stay symmetric at the front. Asymmetry belongs in the `turn` keyframe.
- `turn` = head turned to the viewer's right: the chin and muzzle side move right and the far
  (left) cheek shows more skull. Check the blend at `--turn 0.5` as well as at 1.
- The torso outline is load-bearing: legs blend into it, and tests check that resting legs stay
  inside it. Run the tests after touching it.
- Colours come from the skins. Never put hex colours in the art file.

## Style guide (what "better" means for this dog)

- Chibi proportions: big head, big eyes, short legs, round silhouettes. No sharp corners
  except ear tips and fur scallops.
- Sticker look: one dark warm outline, a bit heavier underneath (`head_ink_drop`). Inner lines
  are thinner and lighter than the silhouette.
- Soft volume: light from the top left. Use the crown shine and rim light on top, soft shade on
  the lower face and a chin shadow on the chest. Keep it subtle: the white muzzle must still read
  as white.
- Fur reads as fur: tufts and scallops instead of straight colour edges.
