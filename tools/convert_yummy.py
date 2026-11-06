"""Yummy Games Core Locomotion, in-place clips, on their own skeleton.

    "C:/Program Files/Blender Foundation/Blender 5.0/blender.exe" --background \
        --factory-startup --python tools/convert_yummy.py

Reads the FBX under asset_sources/ (never written) and writes:

- assets/animations/yummy/locomotion.glb: the source skeleton (UE5 Manny
  topology, no mesh) and one animation per in-place clip, named by its
  semantic id (`ip.standing.walk.f`). The game retargets them at runtime
  onto any character's skeleton, so they are stored once.
- crates/ninja_character/src/yummy_clips.rs: the clip list the game registers
  as graph nodes.

The skeleton is glTF Y-up, +Z forward, in metres, rooted at a `manny` node.
Each clip bakes the armature object's motion into the bones' poses (some
in-place takes turn the object, walk-left by 180 degrees), so the root stays
put. A bone gets a channel only where it moves away from its rest.
"""
import json
import math
from pathlib import Path
import re
import struct
import sys
import tempfile

import bpy
from mathutils import Matrix, Quaternion

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "asset_sources/yummy_games/CoreLocomotionAnimations/Animations"
GLB = ROOT / "assets/animations/yummy/locomotion.glb"
CLIPS_RS = ROOT / "crates/ninja_character/src/yummy_clips.rs"
SKELETON_ROOT = "manny"
# glTF Y-up to Blender Z-up; Blender -Y forward becomes glTF +Z.
C = Matrix(((1, 0, 0, 0), (0, 0, -1, 0), (0, 1, 0, 0), (0, 0, 0, 1)))
CI = C.inverted()
# The audited object bind of every take: centimetres, no turn.
OBJECT_REST = Matrix.Scale(0.01, 4)
# Below these a channel is the rest pose.
EPSILON_M = 1e-4
EPSILON_DOT = 1 - 1e-7
DIRECTIONS = {"f", "fl", "l", "bl", "b", "br", "r", "fr", "lf", "rf"}


def semantic_id(path):
    """`A_IP_Standing_Walk_F.FBX` -> `ip.standing.walk.f` (the old catalog's ids)."""
    tokens = path.stem.rstrip("_").split("_")
    tokens = ["Turn" if t == "Turns" else t for t in tokens]
    return ".".join(t.lower() for t in tokens[1:])


def category_and_title(clip_id):
    """Library category and node title: `standing_walk`, `Walk F Start`."""
    tokens = clip_id.split(".")[1:]
    if tokens[0] == "trans":
        category, rest = "stance_transitions", tokens[1:]
    elif tokens[1] == "trans":
        category, rest = f"{tokens[0]}_transitions", tokens[2:]
    elif len(tokens) > 2 and tokens[2] == "jump":
        category, rest = f"{tokens[0]}_{tokens[1]}_jump", tokens[3:]
    else:
        category, rest = f"{tokens[0]}_{tokens[1]}", tokens[1:]
    words = {"standtocrouch": "Stand To Crouch", "crouchtostand": "Crouch To Stand"}
    title = " ".join(
        t.upper() if t in DIRECTIONS
        else words.get(t, t.replace("2", " To ").title() if re.fullmatch(r"[a-z]+2[a-z]+", t) else t.title())
        for t in rest
    )
    return category, title or tokens[-1].title()


def looping(path):
    meta = path.with_suffix(path.suffix + ".meta").read_text()
    for line in meta.splitlines():
        if line.strip().startswith("loopTime:"):
            return line.split(":")[1].strip() == "1"
    raise ValueError(f"{path.name}: no loopTime")


def import_take(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=str(path), use_anim=True)
    arms = [o for o in bpy.data.objects if o.type == "ARMATURE"]
    if len(arms) != 1:
        raise ValueError(f"{path.name}: expected one armature, found {len(arms)}")
    arm = arms[0]
    if arm.animation_data is None or arm.animation_data.action is None:
        raise ValueError(f"{path.name}: no animation")
    return arm, arm.animation_data.action


def gltf_space(blender):
    """A Blender model-space bone matrix in glTF axes, without scale."""
    m = CI @ blender @ C
    return Matrix.LocRotScale(m.translation, m.to_3x3().normalized().to_quaternion(), (1, 1, 1))


def ordered_bones(arm):
    """Parents before children."""
    order = []

    def visit(bone):
        order.append(bone)
        for child in bone.children:
            visit(child)

    for bone in arm.data.bones:
        if bone.parent is None:
            visit(bone)
    return order


def local(models, bone):
    parent = models[bone.parent.name] if bone.parent else Matrix.Identity(4)
    return parent.inverted() @ models[bone.name]


def append_accessor(doc, data, values, kind):
    flat = [v for row in values for v in row]
    if not all(math.isfinite(v) for v in flat):
        raise ValueError("non-finite animation channel")
    while len(data) % 4:
        data.append(0)
    content = struct.pack("<" + "f" * len(flat), *flat)
    doc["bufferViews"].append(dict(buffer=0, byteOffset=len(data), byteLength=len(content)))
    data.extend(content)
    accessor = dict(bufferView=len(doc["bufferViews"]) - 1, componentType=5126,
                    count=len(values), type=kind)
    if kind == "SCALAR":
        accessor.update(min=[min(flat)], max=[max(flat)])
    doc["accessors"].append(accessor)
    return len(doc["accessors"]) - 1


def write_glb(doc, data, path):
    doc["buffers"] = [{"byteLength": len(data)}]
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    data += b"\0" * (-len(data) % 4)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, suffix=".glb", delete=False) as output:
        temporary = Path(output.name)
        output.write(struct.pack("<III", 0x46546C67, 2, 28 + len(js) + len(data))
                     + struct.pack("<II", len(js), 0x4E4F534A) + js
                     + struct.pack("<II", len(data), 0x004E4942) + data)
    temporary.replace(path)


def skeleton(arm, doc):
    """The rest skeleton as glTF nodes; bone name -> (node index, rest local)."""
    rest_models = {b.name: gltf_space(OBJECT_REST @ b.matrix_local) for b in arm.data.bones}
    doc["nodes"] = [{"name": SKELETON_ROOT, "children": []}]
    nodes = {}
    for bone in ordered_bones(arm):
        rest = local(rest_models, bone)
        t, q = rest.translation, rest.to_quaternion()
        nodes[bone.name] = (len(doc["nodes"]), rest)
        doc["nodes"].append(dict(name=bone.name, translation=list(t),
                                 rotation=[q.x, q.y, q.z, q.w]))
        parent = doc["nodes"][nodes[bone.parent.name][0]] if bone.parent else doc["nodes"][0]
        parent.setdefault("children", []).append(nodes[bone.name][0])
    return nodes


def bake(arm, action, nodes, doc, data, name):
    fps = bpy.context.scene.render.fps / bpy.context.scene.render.fps_base
    start, end = (int(round(f)) for f in action.frame_range)
    bones = ordered_bones(arm)
    if {b.name for b in bones} != set(nodes):
        raise ValueError("skeleton differs from the first take's")
    times, translations, rotations = [], {b.name: [] for b in bones}, {b.name: [] for b in bones}
    for frame in range(start, end + 1):
        bpy.context.scene.frame_set(frame)
        models = {b.name: gltf_space(arm.matrix_world @ arm.pose.bones[b.name].matrix) for b in bones}
        times.append([(frame - start) / fps])
        for bone in bones:
            m = local(models, bone)
            q = m.to_quaternion()
            previous = rotations[bone.name]
            if previous and Quaternion((previous[-1][3], *previous[-1][:3])).dot(q) < 0:
                q.negate()
            translations[bone.name].append(list(m.translation))
            previous.append([q.x, q.y, q.z, q.w])
    animation = {"name": name, "channels": [], "samplers": []}
    time_accessor = append_accessor(doc, data, times, "SCALAR")
    for bone in bones:
        index, rest = nodes[bone.name]
        rest_q = rest.to_quaternion()
        moves = any((Quaternion((w, x, y, z)).dot(rest_q) ** 2) < EPSILON_DOT
                    for x, y, z, w in rotations[bone.name])
        travels = any((rest.translation - type(rest.translation)(t)).length > EPSILON_M
                      for t in translations[bone.name])
        for path, values, kind, used in [("translation", translations[bone.name], "VEC3", travels),
                                         ("rotation", rotations[bone.name], "VEC4", moves)]:
            if not used:
                continue
            animation["samplers"].append(dict(input=time_accessor, interpolation="LINEAR",
                                              output=append_accessor(doc, data, values, kind)))
            animation["channels"].append(dict(sampler=len(animation["samplers"]) - 1,
                                              target=dict(node=index, path=path)))
    doc["animations"].append(animation)
    return times[-1][0]


def rust_list(clips):
    lines = [
        "//! Generated by `tools/convert_yummy.py`; do not edit.",
        "//!",
        "//! The in-place clips of `assets/animations/yummy/locomotion.glb`.",
        "",
        "use super::YummyClip;",
        "",
        "pub(crate) const CLIPS: &[YummyClip] = &[",
    ]
    for clip in clips:
        lines.append(
            f'    YummyClip {{ id: "{clip["id"]}", category: "{clip["category"]}", '
            f'title: "{clip["title"]}", looping: {str(clip["looping"]).lower()}, '
            f'duration: {clip["duration"]:.4f} }},'
        )
    lines.append("];")
    return "\n".join(lines) + "\n"


def main():
    files = sorted(p for p in SOURCE.rglob("A_IP_*") if p.suffix.lower() == ".fbx"
                   and "Climb" not in p.stem)
    doc = {"asset": {"version": "2.0", "generator": "tools/convert_yummy.py"},
           "scene": 0, "scenes": [{"nodes": [0]}], "animations": [],
           "accessors": [], "bufferViews": []}
    data = bytearray()
    nodes, clips = None, []
    for number, path in enumerate(files):
        arm, action = import_take(path)
        if nodes is None:
            nodes = skeleton(arm, doc)
        clip_id = semantic_id(path)
        duration = bake(arm, action, nodes, doc, data, clip_id)
        category, title = category_and_title(clip_id)
        clips.append(dict(id=clip_id, category=category, title=title,
                          looping=looping(path), duration=duration))
        print(f"CLIP {number + 1}/{len(files)} {clip_id} -> {category}: {title}", flush=True)
    if len({c["id"] for c in clips}) != len(clips):
        raise ValueError("clip id collision")
    write_glb(doc, data, GLB)
    CLIPS_RS.write_text(rust_list(clips), encoding="utf-8", newline="\n")
    print(f"WROTE {len(clips)} clips, {GLB.stat().st_size / 1e6:.1f} MB", flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        import traceback
        traceback.print_exc()
        sys.exit(1)
