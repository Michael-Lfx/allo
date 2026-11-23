"""Rebuild original geometric preview fixtures (Pillow and python-pptx)."""
from pathlib import Path
from PIL import Image, ImageDraw
from pptx import Presentation
from pptx.util import Inches, Pt
from pptx.dml.color import RGBColor

ROOT = Path(__file__).parent
image = Image.new("RGB", (1280, 720), "#edf1f3")
draw = ImageDraw.Draw(image)
draw.rectangle((0, 0, 1280, 130), fill="#183b4e")
draw.text((60, 60), "LOCAL FILE PREVIEW", fill="white", font_size=36)
for x, color in [(60, "#d45736"), (450, "#378f91"), (840, "#d9ad47")]:
    draw.rounded_rectangle((x, 200, x + 330, 560), 20, fill=color)
draw.text((60, 640), "Image fixture / RGB channel and aspect-ratio check", fill="#183b4e", font_size=24)
for ext in ["png", "jpg", "webp"]:
    image.save(ROOT / ("sample." + ext))

deck = Presentation()
deck.slide_width, deck.slide_height = Inches(12.8), Inches(7.2)
for number, title in enumerate(["资料库预览", "Images and shapes", "第三页 / Page three"], 1):
    slide = deck.slides.add_slide(deck.slide_layouts[6])
    box = slide.shapes.add_textbox(Inches(0.6), Inches(0.5), Inches(11.6), Inches(1))
    run = box.text_frame.paragraphs[0].add_run()
    run.text = title
    run.font.name, run.font.size = "Microsoft YaHei", Pt(32)
    run.font.color.rgb = RGBColor.from_string("183B4E")
    slide.shapes.add_picture(str(ROOT / "sample.png"), Inches(0.6), Inches(1.8), width=Inches(7.8))
    body = slide.shapes.add_textbox(Inches(8.8), Inches(2.0), Inches(3.4), Inches(3.6))
    body.text_frame.word_wrap = True
    p = body.text_frame.paragraphs[0]
    p.text = "中文与 English\nLocal files stay on this device.\nPage " + str(number)
    p.font.name, p.font.size = "Microsoft YaHei", Pt(20)
deck.save(ROOT / "sample.pptx")

# Small PDF with real text, paths and three pages; no third-party font bytes.
objects = []
def obj(data):
    objects.append(data.encode("ascii") if isinstance(data, str) else data)
    return len(objects)
obj("<< /Type /Catalog /Pages 2 0 R >>")
obj("<< /Type /Pages /Kids [4 0 R 6 0 R 8 0 R] /Count 3 >>")
obj("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
for i in range(3):
    content_id = 5 + i * 2
    obj(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>")
    stream = f"0.09 0.23 0.31 rg 0 660 612 132 re f 1 1 1 rg BT /F1 26 Tf 48 720 Td (Local PDF preview - page {i+1}) Tj ET 0.84 0.30 0.20 rg 48 350 220 220 re f 0.20 0.56 0.57 rg 300 350 220 220 re f 0 0 0 rg BT /F1 16 Tf 48 280 Td (Text, vector shapes, and pagination.) Tj ET".encode("ascii")
    obj(b"<< /Length " + str(len(stream)).encode("ascii") + b" >>\nstream\n" + stream + b"\nendstream")
pdf = bytearray(b"%PDF-1.4\n")
offsets = [0]
for i, data in enumerate(objects, 1):
    offsets.append(len(pdf)); pdf.extend(f"{i} 0 obj\n".encode() + data + b"\nendobj\n")
xref = len(pdf)
pdf.extend(f"xref\n0 {len(objects)+1}\n0000000000 65535 f \n".encode())
for offset in offsets[1:]:
    pdf.extend(f"{offset:010} 00000 n \n".encode())
pdf.extend(f"trailer\n<< /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
(ROOT / "sample.pdf").write_bytes(pdf)
