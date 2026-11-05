from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

HERE = Path(__file__).resolve().parent
EM = 1000
ASCENT = 800
DESCENT = -200
EPOCH = 2082844800


def build(name, characters, mono):
    cmap = {point: f"u{point:04X}" for point in characters}
    order = [".notdef", *cmap.values()]
    glyphs = {}
    metrics = {}
    for glyph in order:
        point = next((point for point, value in cmap.items() if value == glyph), 0)
        width = 600 if mono else 900 if point == ord("W") else 300 if point in map(ord, "il1 ") else 600
        pen = TTGlyphPen(None)
        if point != ord(" "):
            pen.moveTo((50, 0))
            pen.lineTo((width - 50, 0))
            pen.lineTo((width - 50, 700))
            pen.lineTo((50, 700))
            pen.closePath()
        glyphs[glyph] = pen.glyph()
        metrics[glyph] = (width, 0)
    font = FontBuilder(EM, isTTF=True)
    font.setupGlyphOrder(order)
    font.setupCharacterMap(cmap)
    font.setupGlyf(glyphs)
    font.setupHorizontalMetrics(metrics)
    font.setupHorizontalHeader(ascent=ASCENT, descent=DESCENT)
    font.setupNameTable({"familyName": f"Skwd test {name}", "styleName": "Regular", "uniqueFontIdentifier": f"Skwd-test-{name}-1", "fullName": f"Skwd test {name}", "psName": f"SkwdTest-{name}", "version": "Version 1.0"})
    font.setupOS2(sTypoAscender=ASCENT, sTypoDescender=DESCENT, usWinAscent=ASCENT, usWinDescent=-DESCENT)
    font.setupPost()
    font.setupMaxp()
    font.font["head"].created = EPOCH
    font.font["head"].modified = EPOCH
    font.font.recalcTimestamp = False
    font.save(HERE / f"{name}.ttf")


if __name__ == "__main__":
    build("latin", range(32, 127), False)
    build("mono", range(32, 127), True)
    build("fallback", [32, 0x05D0, 0x05D1], False)
