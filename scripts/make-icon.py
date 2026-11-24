#!/usr/bin/env python3
"""生成 SSH7 应用图标源图（1024×1024 PNG，macOS squircle 圆角）。

视觉语言沿用 cindy.cn：Ink 黑底 + 信号红 + 奶白点缀 + 网格叠层 + 裁切角 + 十字准星。

构成：
- 几何「7」——顶栏右上 45° 裁切角、竖干左右边平行等宽、底脚左下 45° 裁切角
- 底部左侧小号等宽「// SSH」字标（红 // + 奶白 SSH），呼应界面里的 // 元数据标签
- 底部右侧细十字准星，呼应 cindy.cn 的取景器元素
- 整图套 macOS superellipse（squircle）遮罩，角落透明，贴近系统原生图标观感
"""

import os

import numpy as np
from PIL import Image, ImageDraw, ImageFont

S = 1024
INK = (8, 8, 10)
RED = (247, 1, 33)
RED_TOP = (255, 12, 44)      # 「7」顶端，比品牌红略亮一档
RED_BOT = (228, 0, 26)       # 「7」底端，比品牌红略暗一档
CREAM = (242, 239, 230)
CREAM_SOFT = (186, 182, 173)
CREAM_DIM = (108, 105, 99)
LINE = (19, 19, 24)

# ── 字体 ──────────────────────────────────────────────
# 优先用 SF Mono（与 App 的等宽字体栈一致），逐个回退。
FONT_CANDIDATES = [
    "/System/Library/Fonts/SFNSMono.ttf",
    "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
    "/System/Library/Fonts/Menlo.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "C:/Windows/Fonts/consola.ttf",
]


def load_font(size: int):
    for path in FONT_CANDIDATES:
        if os.path.exists(path):
            try:
                return ImageFont.truetype(path, size)
            except OSError:
                continue
    return None


# ── 背景：Ink + 网格 + 径向渐晕 ───────────────────────
img = Image.new("RGB", (S, S), INK)
d = ImageDraw.Draw(img)

GRID = 48
for x in range(0, S + 1, GRID):
    d.line([(x, 0), (x, S)], fill=LINE, width=2)
for y in range(0, S + 1, GRID):
    d.line([(0, y), (S, y)], fill=LINE, width=2)

vignette = Image.radial_gradient("L").resize((S, S), Image.LANCZOS)
vignette = vignette.point(lambda v: int(v * 0.55))
img = Image.composite(Image.new("RGB", (S, S), (0, 0, 0)), img, vignette)
d = ImageDraw.Draw(img)

# ── 几何「7」─────────────────────────────────────────
BAR_TOP = 196          # 顶栏上沿
BAR_BOT = 336          # 顶栏下沿（厚度 140）
X_LEFT = 196           # 顶栏左端
X_RIGHT = 840          # 顶栏右端
CH_TOP = 56            # 顶栏右上 45° 裁切角
CH_FOOT = 48           # 底脚左下 45° 裁切角
FOOT_Y = 836           # 竖干底部
K = 0.46               # 竖干倾斜系数：每下降 1px 向左平移 K px
STEM_W = 186           # 竖干水平宽度（左右边平行，恒定）


def right_x(y: float) -> float:
    """竖干右边在给定 y 处的 x。"""
    return X_RIGHT - K * (y - BAR_BOT)


def left_x(y: float) -> float:
    """竖干左边在给定 y 处的 x。"""
    return right_x(y) - STEM_W


seven = [
    (X_LEFT, BAR_TOP),                  # 顶栏左上
    (X_RIGHT - CH_TOP, BAR_TOP),        # 顶栏右上（裁切角起点）
    (X_RIGHT, BAR_TOP + CH_TOP),        # 裁切角
    (X_RIGHT, BAR_BOT),                 # 顶栏右下 = 竖干右边起点
    (right_x(FOOT_Y), FOOT_Y),          # 竖干右边到底
    (left_x(FOOT_Y) + CH_FOOT, FOOT_Y), # 底边
    (left_x(FOOT_Y - CH_FOOT), FOOT_Y - CH_FOOT),  # 底脚左下斜切
    (left_x(BAR_BOT), BAR_BOT),         # 竖干左边回到顶栏下沿
    (X_LEFT, BAR_BOT),                  # 顶栏下沿左端
]

# 「7」用一层极轻的纵向渐变，避免大色块发闷
shape = Image.new("L", (S, S), 0)
ImageDraw.Draw(shape).polygon(seven, fill=255)

grad = np.zeros((S, S, 3), dtype=np.float64)
span = max(FOOT_Y - BAR_TOP, 1)
for i in range(S):
    t = min(max((i - BAR_TOP) / span, 0.0), 1.0)
    grad[i, :, :] = np.array(RED_TOP) * (1 - t) + np.array(RED_BOT) * t
grad_img = Image.fromarray(grad.astype(np.uint8))
img.paste(grad_img, (0, 0), shape)
d = ImageDraw.Draw(img)

# ── 左下角「// SSH」小字标 ────────────────────────────
FONT_SIZE = 74
TRACKING = 6
BASELINE = 952
TAG_X = X_LEFT

font = load_font(FONT_SIZE)
if font is not None:
    def tracked(draw, x, y, text, fnt, fill, track):
        for ch in text:
            draw.text((x, y), ch, font=fnt, fill=fill, anchor="ls")
            x += draw.textlength(ch, font=fnt) + track
        return x

    x = tracked(d, TAG_X, BASELINE, "//", font, RED, TRACKING)
    x += TRACKING
    tracked(d, x, BASELINE, "SSH", font, CREAM_SOFT, TRACKING)
else:
    print("警告：没有找到等宽字体，跳过 // SSH 字标")

# ── 右下角细十字准星 ──────────────────────────────────
CX, CY = 796, 920
ARM, STROKE = 44, 6
d.rectangle([CX - ARM, CY - STROKE // 2, CX + ARM, CY + STROKE // 2], fill=CREAM_DIM)
d.rectangle([CX - STROKE // 2, CY - ARM, CX + STROKE // 2, CY + ARM], fill=CREAM_DIM)

# ── macOS squircle 遮罩（superellipse，n=5）──────────
def squircle_alpha(size: int, n: float = 5.0) -> np.ndarray:
    a = size / 2.0
    ys, xs = np.mgrid[0:size, 0:size].astype(np.float64)
    dx = np.abs(xs + 0.5 - a) / a
    dy = np.abs(ys + 0.5 - a) / a
    f = dx**n + dy**n
    gx = n * dx ** (n - 1) / a
    gy = n * dy ** (n - 1) / a
    grad_mag = np.sqrt(gx**2 + gy**2) + 1e-9
    dist = (f - 1.0) / grad_mag          # 负值=内部
    return np.clip(0.5 - dist, 0.0, 1.0)


alpha = squircle_alpha(S)
rgba = img.convert("RGBA")
rgba.putalpha(Image.fromarray((alpha * 255).astype(np.uint8)))
rgba.save("icon-source.png", "PNG")
print("icon-source.png written", rgba.size)

# ── 预览：把几个关键尺寸拼在一张图里，方便肉眼检查 ──
PREVIEWS = [32, 64, 128, 256, 512]
BG = (38, 38, 44)
strip = Image.new("RGB", (sum(PREVIEWS) + 40 * len(PREVIEWS) + 20, 572), BG)
x = 20
for size in PREVIEWS:
    small = rgba.resize((size, size), Image.LANCZOS)
    strip.paste(small, (x, (560 - size) // 2), small)
    x += size + 40
strip.save("icon-preview.png", "PNG")
print("icon-preview.png written", strip.size)
