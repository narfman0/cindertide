# Cindertide — Animation Integration Notes

Reference for when an animation pack is acquired and integrated.

## Current state

Phase 1 (per-character looping Take 001) + Phase 2 (Transform-overlay state
cues) + Phase 3 (distance-based LOD) ship today. Phase 4 (state-driven
multi-clip animation) is scaffolded in `src/bin/client.rs` —
`try_load_shared_anim_graph` activates it automatically when the four
configured GLBs from `SHARED_ANIM_FILES` exist on disk.

See `feat(animation): Phase 4 — state-driven scaffold` commit for details.

## Reference skeleton (Synty POLYGON characters)

Audited 2026-05 from cached GLBs. **All Synty POLYGON characters share one
50-joint humanoid rig.** Verified across CyberCity human soldier, cyborg
male, and female soldier — identical bone names, identical joint counts.

The full bone list:

```
Root, Hips, Spine_01, Spine_02, Spine_03, Neck, Head, Eyes, Eyebrows, Jaw,

Left arm:
  Clavicle_L, Shoulder_L, Elbow_L, Hand_L,
  Thumb_01, Thumb_02, Thumb_03,
  IndexFinger_01, IndexFinger_02, IndexFinger_03, IndexFinger_04,
  Finger_01, Finger_02, Finger_03, Finger_04,

Right arm (note the `.001` suffix):
  Clavicle_R, Shoulder_R, Elbow_R, Hand_R,
  Thumb_01.001, Thumb_02.001, Thumb_03.001,
  IndexFinger_01.001, IndexFinger_02.001, IndexFinger_03.001, IndexFinger_04.001,
  Finger_01.001, Finger_02.001, Finger_03.001, Finger_04.001,

Right leg: UpperLeg_R, LowerLeg_R, Ankle_R, Ball_R, Toes_R,
Left leg:  UpperLeg_L, LowerLeg_L, Ankle_L, Ball_L, Toes_L
```

## Yellow flag: the `.001` suffix on right-hand finger bones

The right-arm finger and thumb bones in our cached GLBs use Blender's standard
**duplicate-name suffix**: `Thumb_01.001`, `IndexFinger_01.001`, etc. — instead
of the canonical Synty naming (`Thumb_01_R` or `R_Thumb_01`).

This is an artifact of how our `convert_fbx_to_gltf.py` script handles the FBX
import in Blender. When the source FBX has two bones with identical names for
L/R sides, Blender silently appends `.001` to the second one rather than
preserving Synty's semantic suffix scheme.

### Impact on an animation pack

A Synty animation pack authored against Synty's **native** rig will have right-
hand finger tracks targeting `Thumb_01_R` (or whatever Synty's canonical name
is) — **those tracks will silently no-op against our `.001`-suffixed bones.**
The hands won't animate on the right side. Other bones (Hips, Spine, arms,
legs, head, left-hand fingers) all use clean names without suffixes and will
animate correctly.

### Visual impact

For an isometric RTS at our default camera scale (28 ortho units), fingers
are sub-pixel detail. The non-animating right-hand fingers will not be
visible. The body, arms, and legs will animate normally — which is what
players actually see.

**Recommendation: ship Phase 4 ignoring the `.001` issue.** Address it only if
the close-up `close_up` framing preset (used for cinematic dialogue moments)
reveals the static right hand and looks odd.

### If you want to fix it cleanly

Two options:

1. **Re-export the character GLBs** with a Blender script that renames
   `.001`-suffixed bones to `_R` before export. One-time work; gets us into
   semantic alignment with Synty's native rig. Likely required if you want
   third-party (Mixamo) animations to work cleanly.

2. **Re-convert the animation pack** with a similar rename to match our
   existing `.001` naming. Inverse of option 1; cheaper if we already have
   character GLBs cached but more fragile (next pack would need the same
   fix-up).

## What to verify when buying a Synty animation pack

- **Compatibility:** product description should explicitly say "compatible
  with POLYGON characters" — that means same rig.
- **Clip list:** minimum useful set: idle, walk, run, attack (or fire/shoot),
  death. Bonus: reload, aim, hit reactions.
- **Format:** one animation per FBX file. Avoid packs sold only as Unity
  Animator Controller assets — those are Unity-specific binaries we can't
  convert.
- **Rig naming:** check screenshots / docs for whether their right-hand bones
  are `_R`-suffixed or `.001`-suffixed. The Synty native packs use `_R`
  (per audit above, our local copies have been re-suffixed by Blender import).

## Integration checklist after purchase

1. Drop the pack's FBX files into srv's `assets/` mount (e.g.
   `assets/POLYGON_Animations_v1/`).
2. Run `~/.openclaw/workspace/asset-server/fetch_and_convert.sh
   POLYGON_Animations_v1` — outputs GLBs to
   `~/.cindertide/assets/converted/POLYGON_Animations_v1/`.
3. Inspect one converted GLB with the audit script
   (`docs/animation-integration.md` for reference paths). Verify:
   - The GLB has an `animations` array with at least one clip.
   - The animation channels target bones by name.
4. Upload the converted GLBs back to srv and refresh `index.json` so other
   developers get the same prefetch behavior.
5. Update `SHARED_ANIM_FILES` in `src/bin/client.rs` to point at the four
   clip GLBs (idle / walk / attack / die).
6. Run `cargo run --bin cindertide`. Look in the log for either:
   - `[animation] shared animation pack loaded (Phase 4 state-driven mode)`
   - `[animation] shared pack not present (missing …); Phase 1 fallback active`
7. If the right-hand fingers are static and visible at your zoom level,
   address per "If you want to fix it cleanly" above.

## Free fallback: Mixamo

Adobe's Mixamo (free) provides a huge library of mocap animations. Process per
clip:

1. Upload one Synty character FBX to mixamo.com (auto-rigger maps the mesh
   to Mixamo's own `mixamorig:*` rig).
2. Browse Mixamo's animation library; pick a clip. Preview attaches the clip
   to the auto-rigged character.
3. Download as FBX **without** Skin, 30fps. The FBX contains the auto-rigged
   skeleton (with `mixamorig:*` bone names) plus the animation.
4. Run our converter with the **`--rename mixamo`** flag, which renames bones
   from `mixamorig:*` to Synty equivalents during the FBX→GLB step:

   ```sh
   blender --background --python convert_fbx_to_gltf.py -- \
     ~/.cindertide/assets/raw/mixamo_clips \
     ~/.cindertide/assets/converted/mixamo_clips \
     --rename mixamo
   ```

5. Drop the converted GLB path into `SHARED_ANIM_FILES` in `src/bin/client.rs`.
   Restart — Phase 4 picks it up.

The `--rename mixamo` preset (see `RENAME_PRESETS` in
`asset-server/convert_fbx_to_gltf.py`) handles the bone-name conversion
including the spine collapse (Mixamo 4-bone spine → Synty 3-bone). About 30
mapping entries. Bones not in the map (mostly Mixamo's extra hand IK targets)
fall through unmapped and silently no-op at runtime — body silhouette
animates correctly.

**Estimated time:** ~5 minutes per Mixamo clip from download to working GLB.
A full state set (idle/walk/run/fire/reload/death/hit) is roughly half an hour.

### Why this works without runtime code changes

Bevy stores `AnimationClip` curves keyed by `AnimationTargetId` — a UUID
computed by hashing the bone's ancestor path (e.g.,
`hash(["Spine_01", "Hips"])`). The IDs are computed at GLB load time from
whatever names the GLB nodes have. By renaming bones during Blender import
**before** glTF export, we get IDs computed against Synty-named paths.
Those IDs match the IDs Bevy assigns to our character's bone entities at
spawn time, and the animation plays through normally.

A runtime rename would require either reversing the UUID (impossible) or
maintaining a parallel "ghost armature" that animates separately and a
per-frame sync system. Doing it once at import time is dramatically simpler.
