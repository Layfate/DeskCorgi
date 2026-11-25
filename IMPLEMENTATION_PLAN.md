# DeskCorgi — Implementation Plan

> A tiny, native, cross-platform desktop dog that sits behind a little keyboard and mirrors your real keyboard and mouse input.
> **Reference project:** [KuroCat](https://github.com/chanke17/KuroCat) (MIT), a native desktop cat doing the same job in Swift / C++17. DeskCorgi follows its architecture and lessons, rewritten in Rust with original dog designs (see §12).
> Built-in characters: **Corgi** (default) and **Shiba Inu**, swappable live from the menu.
> Language: **Rust**.
>
> **Status (2026-11-25):** M0–M3 are done for Windows: `DeskCorgi.exe` is ~950 KB, uses ~14 MB of RAM, animates in step with the display refresh (vsync-paced, e.g. 75/120/144 fps) while active, uses ~0.9% CPU on a 16-thread machine, and has 47 passing tests. Linux and macOS (M4–M6) are next.

---

## 1. Goals

| Goal | Target | KuroCat for comparison |
|---|---|---|
| Binary size | Windows < 1 MB (stretch: ~500 KB), single file, no runtime | ~802 KB `.exe` |
| CPU when idle | < 1% (adaptive frame rate) | adaptive 30 / 20 / 10 fps |
| Memory | < 30 MB | — |
| Input | Global keyboard + mouse, works in games (NKRO / Raw Input level) | Raw Input / IOHIDManager / XInput2 + evdev |
| Platforms | Windows → Linux (X11) → macOS → Linux (Wayland) | all four |
| Privacy | Keystrokes are never stored, logged or sent. Only "a key is down" + counters | same |

**Not in scope for v1:** a skin workshop website, online features, MCP server, plugins.

---

## 2. Character design

### 2.1 How the dogs are drawn (adopted from KuroCat)
KuroCat does **not** use sprite sheets or PNG layers. The pet is **vector-drawn in code every frame** from a small data file (colours + body shape numbers + vector markings). DeskCorgi does the same:

- No artist is needed to ship v1, and the binary stays small (no embedded images).
- Head turning, paws reaching for keys and ear/tail motion come from real geometry, not from swapping frames.
- A new skin is just a data file, so **Corgi and Shiba share one rig and one renderer**. Switching skins only swaps the data.

### 2.2 Corgi (default skin)
- Pembroke look: red/sable and white coat, white blaze down the face, white chest and paws.
- Big upright ears, **short legs** (lower paw rest positions), long body, fluffy "loaf" butt, stub tail (wiggles the whole rear end instead of wagging).

### 2.3 Shiba Inu (skin #2)
- Red coat with *urajiro* (cream cheeks, muzzle, chest, belly), smaller triangular ears, fox-like face, the "smug" look.
- Taller legs, **curled tail** over the back that wags side to side.

### 2.4 Swapping skins
- Tray / right-click menu → **Skin → Corgi / Shiba / (imported skins…)**. The swap happens instantly: the rig keeps its current pose and springs and only the data changes.
- The choice is saved in the config (`skin = "corgi"`). If an imported skin file goes missing, fall back to Corgi.
- Optional: drag a skin file onto the dog to import it (KuroCat does this with `.kurocat` files).

### 2.5 Layout
- The dog faces the viewer, sitting on a cushion behind a small keyboard, with the mouse on **the dog's right paw (the viewer's left)**, mirroring how you sit at your own desk. KuroCat uses the same 180° mirrored layout.
- One fixed logical canvas (KuroCat uses **360 × 320** units, y down). Everything is scaled to the user's size setting and DPI at render time.

### 2.6 Animation states

| State | Trigger | What the dog does | KuroCat equivalent |
|---|---|---|---|
| `Idle` | No input for a few seconds | Breathes (body squash), blinks, slow tail wag / butt wiggle | idle |
| `Typing` | Key down/up | Left/right paws tap the matching keys on the mini keyboard; floating keycap badge (optional) | typing |
| `Mousing` | Mouse move / click | Right paw on the mouse, mouse moves on its pad, clicks press the buttons | mouse paw |
| `LookAt` | Cursor moves | Head yaw/pitch toward the cursor with 2.5D "dome" projection; eyes lead the head | spherical head tracking |
| `ExcitedTyping` | High WPM | Ears perk up, tail wags faster | fast-typing |
| `ExtraPaw` | 3 keys held at once / 4 keys | Lifts one back paw (showing its toe beans) onto the 3rd key, and the other back paw onto the 4th | 3-key hind paw, 4-key tail press |
| `Chase` | Fast mouse shaking | Play bow, pupils go big and round, tail wags hard, eyes track the cursor like a ball | hunting mode / boba eyes |
| `Zoomies` | Click spam (e.g. 8+ clicks in 1 s) | Short spin / bounce | — (new) |
| `Pat` | User strokes the head | Eyes close happily, sparkles, tail goes wild | petting / purr sparkles |
| `Scruff` | Drag the dog by the back | Body stretches and swings like a long dog, springs back on release | long-cat drag |
| `Sleep` | No input for 5+ minutes | Head rests on the keyboard, floating "z" | sleep |

**Signature mechanic:** tail wag speed (Shiba) / butt wiggle speed (Corgi) follows your live typing speed (WPM).

---

## 3. Skin format

### 3.1 Principles (copied from KuroCat's `docs/skin-format.md`)
- One UTF-8 text file per skin, with a size cap (KuroCat: 2 MB).
- **Numbers are clamped** to their legal range, **unknown fields are ignored** (forward compatible), and **type errors / bad colours / missing required fields reject the whole file** with a clear message.
- A `format_version` field. A newer version is rejected with "please update DeskCorgi".
- Built-in skins are written in code and also shipped as template files, so community skins can start from them.

### 3.2 Coordinate system: part spaces
Every coordinate belongs to a **part** and moves with that part, so markings follow the head when it turns.

| Space | Origin | Follows |
|---|---|---|
| `body` | Cushion anchor (bottom centre of the dog) | Body tilt, breathing squash, `body_width` |
| `head` | Head centre | Yaw / pitch / tilt, projected onto a dome for 2.5D |
| `skull` | Head centre | Tilt and offset only, no 3D turn (big top-of-head patches) |
| `muzzle` | Snout tip area | Head turn with extra parallax (dog-specific: cats have no snout) |
| `tail` | x = 0 root → 100 tip, y = offset across the tail | Wag, curl, flick |
| `ear_l` / `ear_r` | Ear base → tip box, auto-mirrored | Head turn, perk / fold, twitch |

Head 2.5D: map head-space points onto an ellipsoid (`z = depth · √(1 − x²/a² − y²/b²)`), rotate by yaw then pitch, orthographic project, then apply tilt and scale. A `plane` option is available for flat face markings. The jaw line separates head regions from body regions, so the two colours meet on a natural chin line.

### 3.3 Fields
The full, commented templates are `skins/corgi.toml` and `skins/shiba.toml` (generated from the built-ins). In short:
```toml
format_version = 1
name   = "Corgi"
author = "DeskCorgi"

[palette]          # only `body` is required; the rest are derived if missing
body = "#E9933F"   # also: body_shade, belly, legs, paws, pads, ear_outer, ear_inner, nose, mouth,
belly = "#FFF8EE"  #       tongue, tail, tail_tip, outline, blush, markings, iris

[shape]            # numbers are clamped to their range
body_width = 1.12   # also: leg_length, head_scale, muzzle_length, ear_size, ear_spread, ear_pointiness,
leg_length = 0.6    #       eye_size, eye_lid, eye_slant, cheek_fluff, butt_fluff, tail_length, sock_length
tail_style = "stub" # stub | curl | plume
face = "blaze"      # none | blaze | urajiro
chest = "ruff"      # none | ruff | bib
brows = false       # Shiba eyebrow dots

[[regions]]        # closed cubic Bézier shapes, clipped to their part
space = "head"     # head | skull | muzzle | body | tail | ear_l | ear_r
fill = "#FFFFFF"
anchors = [ { x = 0, y = -30 }, { x = 8, y = 10, in_x = 4, in_y = -5, out_x = 12, out_y = 20 } ]

[[marks]]          # polygons or rounded strokes
space = "head"
kind = "polygon"   # polygon | stroke
color = "#3A2414"
points = [[-14, -16], [-10, -18], [-9, -14]]
```
Draw order: base colour → regions → marks → eyes / nose → paws. Optional later: small PNG stickers anchored to `head_top`, `face`, `neck` or `tail_tip` (a collar, a hat), like KuroCat's `decorations`.

### 3.4 Where skins live
- Corgi and Shiba are **built into the binary** as Rust constants (no file I/O needed).
- Imported skins are validated, then copied to the config dir under a content hash (KuroCat uses FNV-1a 64): `%APPDATA%\DeskCorgi\skins\`, `~/.local/share/deskcorgi/skins/`, `~/Library/Application Support/DeskCorgi/skins/`.

---

## 4. Architecture

### 4.1 Workspace layout
```
deskcorgi/
  Cargo.toml                 # workspace
  crates/
    core/                    # pure Rust, no OS code, fully unit-tested
      src/spring.rs          # Spring1D / Spring2D (all motion is springs)
      src/rig.rs             # pose: paws, head yaw/pitch/tilt, ears, tail, body squash
      src/state.rs           # animation state machine (§2.6)
      src/input.rs           # InputEvent enum, pressed-key set, WPM counter
      src/keymap.rs          # PhysicalKey enum, mini-keyboard layout, which paw presses which key
      src/skin.rs            # skin parsing + validation + built-in Corgi/Shiba specs
      src/render.rs          # draws the dog into an RGBA buffer (tiny-skia paths)
      src/config.rs          # settings (skin, size, eye colour, fps mode, toggles)
    platform-windows/        # windows-rs
    platform-linux/          # x11rb + XInput2, evdev for Wayland
    platform-macos/          # objc2, core-graphics, IOHIDManager
  app/                       # main.rs: picks the platform crate, runs the loop
  skins/corgi.toml  skins/shiba.toml   # templates (same data as the built-ins)
  .github/workflows/release.yml
```

**Key idea:** `core` renders each frame into a plain RGBA pixel buffer. Each platform crate only has to (1) create a transparent always-on-top window, (2) copy the buffer to it, and (3) send input events to `core`. KuroCat duplicates its renderer three times (Swift, Windows C++, Linux C++), about 6,000 lines each. DeskCorgi avoids that by keeping **one** renderer in `core`.

### 4.2 Data flow
```
OS input thread ──InputEvent──► channel ──► core::State::update(dt)
                                               │
                                   core::render(frame) → RGBA buffer (premultiplied)
                                               │
                              platform::present(buffer) → screen
```

`InputEvent` holds only `KeyDown(PhysicalKey)`, `KeyUp(PhysicalKey)`, `MouseMove(dx, dy)`, `Click(button)`, `Scroll`. Key codes are mapped to a `PhysicalKey` (used to pick the paw and light up a key), then thrown away.

### 4.3 Motion model (from KuroCat `CatPhysics`)
- Every moving thing is a **damped spring** with `value`, `velocity`, `target`: `v += ((target − value)·k − v·c)·dt; value += v·dt`. KuroCat's defaults are k≈260 / c≈22 (1D) and k≈340 / c≈25 (2D). Clamp `dt` so a frame hitch can't explode the springs.
- The state machine only sets **targets** (paw → key position, head yaw → cursor angle, pupil scale → 1.6 in Chase), and the springs make it look smooth.
- Separate springs for: left/right paw, back paw, tail tip, tail curl, body shift / tilt / squash, head yaw / pitch / tilt / offset, ear perk / twitch, eye gaze, pupil size, chase intensity, scruff stretch, mouse offset, mouse-button press.

### 4.4 Crates

| Need | Crate |
|---|---|
| 2D vector drawing | `tiny-skia` (paths, gradients, anti-aliasing) |
| Skin / config files | `serde` + `toml` |
| Windows API | `windows` (windows-rs) |
| Linux X11 | `x11rb` (with XInput2 + shm features) |
| Linux Wayland input | `evdev` |
| macOS | `objc2`, `objc2-app-kit`, `core-graphics`, `core-foundation`, IOKit bindings |
| Tray icon | raw `Shell_NotifyIcon` / `NSStatusItem` (smaller than `tray-icon`) |
| PNG (only for optional stickers, later) | `png` |

Avoid `winit` and GUI frameworks on Windows to keep the binary small.

---

## 5. Platform details

### 5.1 Windows (first target)
- **Window:** `WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, drawn with `UpdateLayeredWindow` from a 32-bit premultiplied DIB.
- **Click-through:** transparent pixels pass clicks through automatically. **Full click-through** mode adds `WS_EX_TRANSPARENT` for gaming. Ways back out, as in KuroCat: global hotkey (e.g. `Ctrl+Alt+Shift+D`), hold `Alt` to right-click / drag the dog, the tray icon, or launching the `.exe` again.
- **Input:** Raw Input with `RIDEV_INPUTSINK` for keyboard and mouse, so it keeps working when other apps or games have focus. Map make-code + `RI_KEY_E0` flags to `PhysicalKey`.
- **Robustness (lessons from KuroCat):**
  - Re-create or repair the window and re-register Raw Input after Win+L unlock, sleep/resume and display-mode changes.
  - If a fullscreen app is active and no raw keys arrive, periodically re-register `RIDEV_INPUTSINK`.
  - Single-instance mutex: a second launch tells the first to reset click-through and open its menu.
  - **Restart as administrator** menu item, because elevated games hide keystrokes from normal apps.
  - Document that exclusive-fullscreen games hide overlays (use borderless).
- **Stream mode:** a solid `#00FF00` background that **turns off `WS_EX_LAYERED`**, because OBS window capture shows layered windows as black.
- **Autostart:** `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- **DPI:** per-monitor DPI awareness v2 in the manifest.
- Target Windows 10/11 (Rust's MSVC target no longer supports Windows 7; KuroCat supports 7 SP1).

### 5.2 Linux X11
- 32-bit ARGB visual window, `_NET_WM_STATE_ABOVE`, input shape set to empty for click-through.
- Input via XInput2 raw events (`XI_RawKeyPress`, `XI_RawMotion`). This works without `sudo` on X11.
- Package as `.deb`, AppImage and a `tar.gz` with a no-sudo `install.sh` into `~/.local` (KuroCat ships all three). Build on an old distro container or with `cargo-zigbuild` for older glibc. x86_64 + arm64.

### 5.3 macOS
- Borderless `NSWindow`, clear background, `level = .floating`, `ignoresMouseEvents` for click-through. Menu-bar app with no Dock icon.
- Input: **IOHIDManager** (HID layer, unaffected by IME switching, sees standalone modifiers and Fn) plus a listen-only **CGEventTap**, as KuroCat does. Requires **Input Monitoring** permission. Show a friendly first-run prompt with a "Restart DeskCorgi" button.
- While full click-through is on, hold `⌥ Option` to drag or right-click.
- Universal binary (arm64 + x86_64) via `lipo`. Autostart via LaunchAgent / `SMAppService`.

### 5.4 Linux Wayland (last)
- Wayland blocks global input for normal apps. Read `/dev/input/event*` with `evdev`; the user must run `sudo usermod -aG input $USER` and log in again (document this clearly).
- Window: `wlr-layer-shell` (KDE, Sway, Hyprland). GNOME doesn't support it, so fall back to XWayland there.

---

## 6. Frame loop and performance

- Frames are paced by DWM vsync (`DwmFlush`) at the display's refresh rate while the dog is active, and by a high-resolution waitable timer at 30 fps idle and 20 fps asleep. *Balanced* (60) and *Power saver* (30) presets cap this.
- Only re-render when something changed or a spring is still moving. Sleep the thread between frames (no busy loop).
- Cache tessellated paths per skin and size. Rebuild them only when the skin or size changes.

### Release profile
```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```
Check size regularly with `cargo bloat --release`.

---

## 7. Settings (tray + right-click menu)

- **Skin: Corgi / Shiba / imported skins… / Import skin… / Delete skin**, switched live
- Size: 5 levels
- Eye colour (6+ presets), keyboard theme (3 presets)
- Toggles: keycap bubbles, keyboard backlight sweep, chase mode, sleep mode, daily key counter
- Frame rate: Adaptive / 60 fps
- Click-through: Auto / Full, lock position, always on top
- Start with system
- "Stream mode": solid green background for OBS chroma key
- Restart as administrator (Windows)
- Quit

Settings saved to TOML in the OS config dir (`%APPDATA%\DeskCorgi`, `~/.config/deskcorgi`, `~/Library/Application Support/DeskCorgi`).

---

## 8. Milestones

### M0 — Setup (1–2 days)
- [x] Cargo workspace, `core` + `platform-windows` + `app` crates
- [x] CI: build + `cargo test` + `cargo clippy` on Windows/Linux/macOS

### M1 — Windows MVP (1 week)
- [x] Transparent, always-on-top, draggable layered window with per-pixel click-through
- [x] Placeholder Corgi from simple tiny-skia shapes
- [x] Raw Input keyboard → paws tap keys on the mini keyboard
- [x] Mouse move → right paw moves the mouse
- [x] Tray icon with Quit

### M2 — Corgi rig + animations (1–2 weeks)
- [x] Vector Corgi: body, head with dome projection, ears, short legs, stub tail / butt wiggle
- [x] Spring rig + state machine: Idle, Typing, Mousing, LookAt, Sleep
- [x] WPM-linked wiggle speed
- [x] ExtraPaw, Pat, Chase, Zoomies, Scruff states
- [x] Adaptive frame rate

### M3 — Skin system + Shiba + swap (1 week)
- [x] Skin format parser + validator (clamp / ignore unknown / reject with message)
- [x] Shiba built-in skin: curled tail, taller legs, urajiro markings
- [x] Live **Skin → Corgi / Shiba** switch in the menu, remembered in config
- [x] Import skin (menu + drag onto the dog), delete skin
- [x] Settings menu + config file + autostart, Windows robustness items from §5.1

### M4 — Linux X11 (1 week)
- [ ] ARGB window + XInput2 input
- [ ] `.deb`, AppImage and `install.sh` builds

### M5 — macOS (1–2 weeks)
- [ ] NSWindow + IOHIDManager + CGEventTap
- [ ] Permission onboarding prompt
- [ ] Universal binary, signing + notarization

### M6 — Wayland (1 week)
- [ ] evdev input + layer-shell window, XWayland fallback

### M7 — Release (ongoing)
- [ ] GitHub Actions release workflow for all targets (x86_64 + arm64)
- [ ] README (English + Filipino if wanted), GIF demos, OBS guide
- [ ] Code signing on Windows (e.g. SignPath for open source, which KuroCat uses) to reduce SmartScreen warnings

Times are rough estimates for one person working part-time.

---

## 9. Testing

- **Unit tests in `core`:** state transitions with a fake clock, WPM counter, keymap, skin parsing (good file, clamped values, unknown fields, each rejection message), spring stability.
- **Pose snapshot tests:** render *every built-in skin × a fixed set of poses* (idle, typing, head turned, chase, sleep, scruff…) to PNG and compare against saved images. KuroCat does 4 skins × 19 poses byte-for-byte in `tests/linux-render/render_poses.cpp`. Because DeskCorgi has a single renderer, this covers every platform at once.
- **Manual test matrix:** Windows 10/11, Ubuntu (X11 + Wayland), KDE, macOS Intel + Apple Silicon; multiple monitors; high DPI; a fullscreen game; an elevated game; Win+L and sleep/resume; OBS capture.

---

## 10. Privacy rules

1. No keylogging: key codes are used for the current frame only, never saved.
2. Only counters are kept (keys today, clicks), and the user can turn them off.
3. No network calls except an optional update check, which can be disabled.
4. All of this is stated in the README, and the code is open source (MIT).

---

## 11. Risks

| Risk | Mitigation |
|---|---|
| Vector dog looks stiff or "off" | Iterate on shape params with pose snapshots; tune springs; optional PNG stickers for detail |
| Anti-cheat software flags global input | Use Raw Input (read-only, no hooks injected); document it |
| Elevated / fullscreen games hide input or overlay | "Restart as administrator", re-register Raw Input, borderless-fullscreen note |
| macOS permission confusion | Clear first-run prompt with screenshots |
| Wayland differences between desktops | Ship X11/XWayland first, Wayland as "beta" |
| Binary size creeps up | `cargo bloat` in CI, avoid heavy crates |
| Too close to KuroCat | See §12 |

---

## 12. Using KuroCat as the reference

KuroCat is MIT-licensed. DeskCorgi uses it as a **design and behaviour reference**: architecture, input tricks, skin-format rules and edge cases. The code is written fresh in Rust and the characters are original dogs. If any piece is ported closely, keep KuroCat's MIT copyright notice in `THIRD_PARTY_NOTICES` and credit it in the README either way.

Where to look in the KuroCat repo for each DeskCorgi module:

| DeskCorgi module | Study in KuroCat |
|---|---|
| `core/spring.rs`, `core/rig.rs`, `core/state.rs` | `windows/src/CatPhysics.hpp` (springs, rest positions, hunting / scruff / sleep logic) |
| `core/keymap.rs` | `windows/src/KeyMap.hpp` (`PhysicalKey`, Raw Input mapping, which paw presses which key, keyboard layout) |
| `core/render.rs` | `windows/src/CatRenderer.hpp`, `Sources/CatRenderer.swift` (drawing order, head projection, eyes) |
| `core/skin.rs` | `docs/skin-format.md`, `shared/SkinSpec.hpp`, `skins/*.kurocat` |
| `platform-windows` | `windows/src/main.cpp` (layered window, click-through, tray, hotkey, mutex, self-repair), `windows/src/InputEngine.hpp`, `windows/app.manifest` |
| `platform-linux` | `linux/src/InputEngine.hpp` (XInput2 + evdev), `linux/build_deb.sh`, `linux/install.sh` |
| `platform-macos` | `Sources/InputEngine.swift` (IOHIDManager + CGEventTap), `Sources/OverlayWindow.swift`, `build.sh` |
| Tests | `tests/linux-render/render_poses.cpp`, `tests/skin-format/parse_test.cpp` |
| Release / streaming | `.github/workflows/build.yml`, `homepage/obs-guide/` |

---

## 13. Open questions

- Corgi colour variants later (tri-colour, sable, merle)? Shiba variants (black & tan, sesame)?
- Skin file extension: plain `.toml` or a custom `.deskcorgi`?
- License: MIT (same as KuroCat) or other?
