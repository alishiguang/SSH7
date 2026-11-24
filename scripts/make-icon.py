#!/usr/bin/env python3
"""生成 SSH7 应用图标源图（1024×1024 PNG）。

视觉语言沿用 cindy.cn：Ink 黑底 + 信号红 + 奶白点缀 + 网格叠层。
"""

from PIL import Image, ImageDraw

S = 1024
INK = (8, 8, 10)
RED = (247, 1, 33)
CREAM = (242, 239, 230)
LINE = (22, 22, 27)

img = Image.new("RGB", (S, S), INK)
d = ImageDraw.Draw(img)

# 背景网格
for x in range(0, S, 64):
    d.line([(x, 0), (x, S)], fill=LINE, width=2)
for y in range(0, S, 64):
    d.line([(0, y), (S, y)], fill=LINE, width=2)

# 巨型「7」——几何多边形，呼应裁切角风格
seven = [
    (212, 214),
    (828, 214),
    (828, 352),
    (596, 862),
    (398, 862),
    (676, 352),
    (212, 352),
]
d.polygon(seven, fill=RED)

# 「7」顶端的奶白切口，模拟 cindy.cn logo 里的箭头断口
d.polygon(
    [
        (212, 214),
        (300, 214),
        (300, 352),
        (212, 352),
    ],
    fill=CREAM,
)

# 右下角信号点
d.ellipse([(742, 742), (826, 826)], fill=CREAM)

img.save("icon-source.png", "PNG")
print("icon-source.png written", img.size)
