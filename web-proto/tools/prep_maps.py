"""Light, low-saturation treatment of the real 8111 map.img captures (2048x2048, kept square, no resize)."""
from PIL import Image, ImageEnhance
SRC = '/workspace/wt-analysis/'
def treat(src, dst, gamma, sat, contrast, lift, paper=(236, 233, 220)):
    im = Image.open(SRC + src).convert('RGB')
    lut = [int(255 * (i / 255) ** gamma) for i in range(256)] * 3
    im = im.point(lut)
    im = ImageEnhance.Color(im).enhance(sat)
    im = ImageEnhance.Contrast(im).enhance(contrast)
    im = Image.blend(im, Image.new('RGB', im.size, paper), lift)
    im.save('public/maps/' + dst, quality=80, optimize=True, progressive=True)
treat('map_ground_3a7d46abe5a8.png', 'ground_firing_range.jpg', 0.56, 0.55, 0.95, 0.28)
treat('map_air_7e6edd02c5ec.png', 'air_denmark.jpg', 0.40, 0.5, 1.05, 0.30)
