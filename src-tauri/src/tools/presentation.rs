use anyhow::Result;
use async_trait::async_trait;
use piscis_kernel::agent::tool::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const PRESENTATION_TIMEOUT_SECS: u64 = 180;

pub struct PresentationTool;

#[async_trait]
impl Tool for PresentationTool {
    fn name(&self) -> &str {
        "presentation"
    }

    fn description(&self) -> &str {
        "Create modern .pptx presentations from a structured slide specification using python-pptx. \
         This is the preferred tool for creating new PowerPoint files. It does not automate Microsoft PowerPoint or COM. \
         Provide an absolute output path, a concise deck title, an optional subtitle/theme, and a slides array. \
         Each slide may include title, subtitle, bullets, body, image_path, accent, and layout. \
         The tool generates a polished widescreen deck, verifies the output file, and returns slide count and file size."
    }

    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed(
            "Create a modern .pptx from structured JSON. Prefer this for new PPT deliverables instead of Office COM or large generated scripts. Requires path,title,slides.",
        )
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute .pptx output path." },
                "title": { "type": "string", "description": "Deck title." },
                "subtitle": { "type": "string", "description": "Optional cover subtitle." },
                "theme": {
                    "type": "string",
                    "enum": ["tech_dark", "clean_light", "business_blue", "editorial"],
                    "description": "Visual theme. Default tech_dark."
                },
                "slides": {
                    "type": "array",
                    "description": "Slides after the generated cover. Keep each slide concise.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "subtitle": { "type": "string" },
                            "body": { "type": "string" },
                            "bullets": { "type": "array", "items": { "type": "string" } },
                            "layout": {
                                "type": "string",
                                "enum": ["section", "bullets", "two_column", "quote", "image", "summary"]
                            },
                            "image_path": { "type": "string" },
                            "accent": { "type": "string", "description": "Optional hex color such as #2F80ED." }
                        },
                        "required": ["title"]
                    }
                }
            },
            "required": ["path", "title", "slides"]
        })
    }

    fn input_schema_minimal(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "title": { "type": "string" },
                "subtitle": { "type": "string" },
                "theme": { "type": "string", "enum": ["tech_dark", "clean_light", "business_blue", "editorial"] },
                "slides": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "subtitle": { "type": "string" },
                            "body": { "type": "string" },
                            "bullets": { "type": "array", "items": { "type": "string" } },
                            "layout": { "type": "string", "enum": ["section", "bullets", "two_column", "quote", "image", "summary"] },
                            "image_path": { "type": "string" },
                            "accent": { "type": "string" }
                        },
                        "required": ["title"]
                    }
                }
            },
            "required": ["path", "title", "slides"]
        })
    }

    fn needs_confirmation(&self, _input: &Value) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let path_str = match input["path"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
            Some(path) => path,
            None => return Ok(ToolResult::err("Missing required parameter: path")),
        };
        if !path_str.to_lowercase().ends_with(".pptx") {
            return Ok(ToolResult::err("path must end with .pptx"));
        }
        match input["title"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
            Some(_) => {}
            None => return Ok(ToolResult::err("Missing required parameter: title")),
        };
        let slides = match input["slides"].as_array() {
            Some(slides) if !slides.is_empty() => slides,
            _ => return Ok(ToolResult::err("slides must be a non-empty array")),
        };
        if slides.len() > 40 {
            return Ok(ToolResult::err("slides array is too large; split decks above 40 slides"));
        }

        let output_path = resolve_path(path_str, &ctx.workspace_root);
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let work_dir = ctx.workspace_root.join(".workbuddy").join("presentation");
        std::fs::create_dir_all(&work_dir)?;
        let spec_path = work_dir.join(format!("deck_{}.json", uuid::Uuid::new_v4().simple()));
        let script_path = work_dir.join("generate_deck.py");
        std::fs::write(&spec_path, serde_json::to_vec_pretty(&input)?)?;
        std::fs::write(&script_path, generator_script())?;

        let python = find_python();
        let child = Command::new(&python)
            .arg(&script_path)
            .arg(&spec_path)
            .arg(&output_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let output = match timeout(Duration::from_secs(PRESENTATION_TIMEOUT_SECS), child.wait_with_output()).await {
            Ok(result) => result?,
            Err(_) => {
                let _ = std::fs::remove_file(&spec_path);
                return Ok(ToolResult::err(format!(
                    "presentation generation timed out after {}s",
                    PRESENTATION_TIMEOUT_SECS
                )));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() {
            let install_hint = if stderr.contains("No module named 'pptx'") {
                "\npython-pptx is missing. Run: python -m pip install python-pptx"
            } else {
                ""
            };
            let _ = std::fs::remove_file(&spec_path);
            return Ok(ToolResult::err(format!(
                "presentation generator failed (status={}):\nstdout:\n{}\nstderr:\n{}{}",
                output.status, stdout, stderr, install_hint
            )));
        }

        let metadata = match std::fs::metadata(&output_path) {
            Ok(meta) if meta.len() > 0 => meta,
            _ => {
                let _ = std::fs::remove_file(&spec_path);
                return Ok(ToolResult::err(format!(
                    "presentation generator completed but output file was not created or is empty: {}",
                    output_path.display()
                )));
            }
        };

        let _ = std::fs::remove_file(&spec_path);
        Ok(ToolResult::ok(format!(
            "Created presentation: {} ({} slides including cover, {} bytes)\n{}",
            output_path.display(),
            slides.len() + 1,
            metadata.len(),
            stdout
        )))
    }
}

fn resolve_path(path: &str, workspace_root: &Path) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        workspace_root.join(p)
    }
}

fn find_python() -> String {
    std::env::var("PYTHON")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "python".to_string())
}

fn generator_script() -> &'static [u8] {
    br##"# -*- coding: utf-8 -*-
import json, os, sys, math
from pptx import Presentation
from pptx.util import Inches, Pt
from pptx.dml.color import RGBColor
from pptx.enum.text import PP_ALIGN
from pptx.enum.shapes import MSO_SHAPE

spec_path, out_path = sys.argv[1], sys.argv[2]
with open(spec_path, "r", encoding="utf-8") as f:
    spec = json.load(f)

prs = Presentation()
prs.slide_width = Inches(13.333)
prs.slide_height = Inches(7.5)
blank = prs.slide_layouts[6]

themes = {
    "tech_dark": {"bg": (9, 14, 28), "fg": (242, 246, 255), "muted": (155, 168, 190), "accent": (56, 189, 248), "panel": (20, 30, 52)},
    "clean_light": {"bg": (248, 250, 252), "fg": (15, 23, 42), "muted": (71, 85, 105), "accent": (37, 99, 235), "panel": (255, 255, 255)},
    "business_blue": {"bg": (239, 246, 255), "fg": (15, 23, 42), "muted": (71, 85, 105), "accent": (29, 78, 216), "panel": (255, 255, 255)},
    "editorial": {"bg": (250, 250, 249), "fg": (28, 25, 23), "muted": (87, 83, 78), "accent": (190, 24, 93), "panel": (255, 255, 255)}
}
theme = themes.get(spec.get("theme") or "tech_dark", themes["tech_dark"])

def rgb(v): return RGBColor(*v)
def safe_text(v): return str(v or "").strip()
def hex_to_rgb(s, fallback):
    if not isinstance(s, str): return fallback
    s = s.strip().lstrip("#")
    if len(s) != 6: return fallback
    try: return tuple(int(s[i:i+2], 16) for i in (0,2,4))
    except Exception: return fallback

def set_bg(slide, color):
    fill = slide.background.fill
    fill.solid()
    fill.fore_color.rgb = rgb(color)

def add_text(slide, text, x, y, w, h, size=24, bold=False, color=None, align=None):
    box = slide.shapes.add_textbox(Inches(x), Inches(y), Inches(w), Inches(h))
    tf = box.text_frame
    tf.clear()
    tf.word_wrap = True
    p = tf.paragraphs[0]
    p.text = safe_text(text)
    if align is not None:
        p.alignment = align
    run = p.runs[0] if p.runs else p.add_run()
    font = run.font
    font.name = "Microsoft YaHei"
    font.size = Pt(size)
    font.bold = bold
    font.color.rgb = rgb(color or theme["fg"])
    return box

def add_bullets(slide, bullets, x, y, w, h, size=20):
    box = slide.shapes.add_textbox(Inches(x), Inches(y), Inches(w), Inches(h))
    tf = box.text_frame
    tf.clear()
    tf.word_wrap = True
    for idx, item in enumerate([safe_text(b) for b in bullets if safe_text(b)]):
        p = tf.paragraphs[0] if idx == 0 else tf.add_paragraph()
        p.text = item
        p.level = 0
        p.space_after = Pt(8)
        if p.runs:
            r = p.runs[0]
        else:
            r = p.add_run()
        r.font.name = "Microsoft YaHei"
        r.font.size = Pt(size)
        r.font.color.rgb = rgb(theme["fg"])
    return box

def add_panel(slide, x, y, w, h, color=None):
    shp = slide.shapes.add_shape(MSO_SHAPE.ROUNDED_RECTANGLE, Inches(x), Inches(y), Inches(w), Inches(h))
    shp.fill.solid()
    shp.fill.fore_color.rgb = rgb(color or theme["panel"])
    shp.line.color.rgb = rgb(tuple(min(255, c + 25) for c in (color or theme["panel"])))
    return shp

def add_footer(slide, index):
    add_text(slide, f"{index:02d}", 12.1, 6.88, 0.6, 0.25, 9, False, theme["muted"], PP_ALIGN.RIGHT)

def decorate(slide, accent):
    bar = slide.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(0), Inches(0), Inches(0.12), Inches(7.5))
    bar.fill.solid()
    bar.fill.fore_color.rgb = rgb(accent)
    bar.line.fill.background()
    dot = slide.shapes.add_shape(MSO_SHAPE.OVAL, Inches(11.9), Inches(0.42), Inches(0.26), Inches(0.26))
    dot.fill.solid()
    dot.fill.fore_color.rgb = rgb(accent)
    dot.line.fill.background()

def cover():
    slide = prs.slides.add_slide(blank)
    set_bg(slide, theme["bg"])
    accent = theme["accent"]
    decorate(slide, accent)
    add_text(slide, spec.get("title"), 0.8, 1.7, 10.6, 1.1, 42, True, theme["fg"])
    subtitle = safe_text(spec.get("subtitle"))
    if subtitle:
        add_text(slide, subtitle, 0.85, 2.85, 9.6, 0.65, 19, False, theme["muted"])
    add_panel(slide, 0.85, 4.35, 4.2, 0.72, tuple(max(0, c - 5) for c in theme["panel"]))
    add_text(slide, "Harmony WorkBuddy", 1.08, 4.56, 3.4, 0.25, 13, True, accent)
    return slide

def slide_for(item, idx):
    slide = prs.slides.add_slide(blank)
    set_bg(slide, theme["bg"])
    accent = hex_to_rgb(item.get("accent"), theme["accent"])
    decorate(slide, accent)
    layout = item.get("layout") or "bullets"
    title = safe_text(item.get("title"))
    subtitle = safe_text(item.get("subtitle"))
    body = safe_text(item.get("body"))
    bullets = item.get("bullets") or []
    add_text(slide, title, 0.65, 0.52, 10.8, 0.55, 25, True, theme["fg"])
    if subtitle:
        add_text(slide, subtitle, 0.67, 1.08, 10.0, 0.34, 12, False, theme["muted"])
    if layout == "section":
        add_text(slide, title, 0.9, 2.35, 10.8, 0.9, 36, True, theme["fg"])
        if body:
            add_text(slide, body, 0.95, 3.35, 9.6, 1.0, 19, False, theme["muted"])
    elif layout == "two_column":
        add_panel(slide, 0.75, 1.72, 5.65, 4.65)
        add_panel(slide, 6.65, 1.72, 5.65, 4.65)
        mid = max(1, math.ceil(len(bullets) / 2))
        add_bullets(slide, bullets[:mid], 1.05, 2.0, 5.0, 3.8, 18)
        add_bullets(slide, bullets[mid:] or ([body] if body else []), 6.95, 2.0, 5.0, 3.8, 18)
    elif layout == "quote":
        add_text(slide, body or (bullets[0] if bullets else ""), 1.25, 2.35, 10.4, 1.7, 28, True, theme["fg"], PP_ALIGN.CENTER)
    elif layout == "image" and item.get("image_path") and os.path.exists(item.get("image_path")):
        slide.shapes.add_picture(item.get("image_path"), Inches(0.8), Inches(1.65), width=Inches(6.1))
        add_panel(slide, 7.25, 1.65, 4.75, 4.85)
        add_bullets(slide, bullets or ([body] if body else []), 7.55, 1.95, 4.15, 4.1, 17)
    elif layout == "summary":
        cols = [bullets[i:i+3] for i in range(0, len(bullets), 3)] or [[body]]
        for i, col in enumerate(cols[:3]):
            add_panel(slide, 0.82 + i * 4.0, 1.86, 3.55, 4.4)
            add_bullets(slide, col, 1.08 + i * 4.0, 2.18, 3.05, 3.55, 16)
    else:
        add_panel(slide, 0.82, 1.72, 11.35, 4.82)
        if body:
            add_text(slide, body, 1.15, 1.98, 10.6, 0.82, 18, False, theme["muted"])
            add_bullets(slide, bullets, 1.15, 2.95, 10.55, 3.1, 19)
        else:
            add_bullets(slide, bullets, 1.15, 2.02, 10.55, 4.0, 20)
    add_footer(slide, idx)
    return slide

cover()
for idx, slide in enumerate(spec.get("slides") or [], start=2):
    slide_for(slide, idx)

prs.save(out_path)
print(json.dumps({"ok": True, "path": out_path, "slides": len(prs.slides)}, ensure_ascii=False))
"##
}
