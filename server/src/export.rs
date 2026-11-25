//! Small, dependency-free writers for the accountant export: a ZIP (stored, UTF-8 names),
//! semicolon CSV (what Portuguese Excel opens) and a one-font text PDF.

// ───────── ZIP ─────────

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c = t[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// A ZIP archive built in memory. Files are stored (no compression): PDFs and photos don't shrink anyway.
pub struct Zip {
    buf: Vec<u8>,
    central: Vec<u8>,
    count: u16,
    names: std::collections::HashSet<String>,
    dos_time: u16,
    dos_date: u16,
}

impl Zip {
    pub fn new(year: i32, month: u8, day: u8) -> Self {
        let dos_date = (((year - 1980).max(0) as u16) << 9) | ((month as u16) << 5) | day as u16;
        Zip { buf: Vec::new(), central: Vec::new(), count: 0, names: Default::default(), dos_time: 12 << 11, dos_date }
    }

    /// Adds a file; a name already used gets " (2)", " (3)"… before the extension.
    pub fn add(&mut self, name: &str, data: &[u8]) {
        let name = self.unique(name);
        let crc = crc32(data);
        let n = name.as_bytes();
        let offset = self.buf.len() as u32;
        let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        // local header
        le32(&mut self.buf, 0x0403_4b50);
        le16(&mut self.buf, 20);
        le16(&mut self.buf, 1 << 11); // UTF-8 names
        le16(&mut self.buf, 0);
        le16(&mut self.buf, self.dos_time);
        le16(&mut self.buf, self.dos_date);
        le32(&mut self.buf, crc);
        le32(&mut self.buf, data.len() as u32);
        le32(&mut self.buf, data.len() as u32);
        le16(&mut self.buf, n.len() as u16);
        le16(&mut self.buf, 0);
        self.buf.extend_from_slice(n);
        self.buf.extend_from_slice(data);
        // central directory entry
        let c = &mut self.central;
        le32(c, 0x0201_4b50);
        le16(c, 20);
        le16(c, 20);
        le16(c, 1 << 11);
        le16(c, 0);
        le16(c, self.dos_time);
        le16(c, self.dos_date);
        le32(c, crc);
        le32(c, data.len() as u32);
        le32(c, data.len() as u32);
        le16(c, n.len() as u16);
        le16(c, 0);
        le16(c, 0);
        le16(c, 0);
        le16(c, 0);
        le32(c, 0);
        le32(c, offset);
        c.extend_from_slice(n);
        self.count += 1;
    }

    fn unique(&mut self, name: &str) -> String {
        let mut candidate = name.to_string();
        let (stem, ext) = match name.rfind('.') {
            Some(i) if i > name.rfind('/').map(|j| j + 1).unwrap_or(0) => (&name[..i], &name[i..]),
            _ => (name, ""),
        };
        let mut k = 2;
        while self.names.contains(&candidate) {
            candidate = format!("{stem} ({k}){ext}");
            k += 1;
        }
        self.names.insert(candidate.clone());
        candidate
    }

    pub fn finish(mut self) -> Vec<u8> {
        let start = self.buf.len() as u32;
        let size = self.central.len() as u32;
        self.buf.extend_from_slice(&self.central);
        let b = &mut self.buf;
        b.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(&self.count.to_le_bytes());
        b.extend_from_slice(&self.count.to_le_bytes());
        b.extend_from_slice(&size.to_le_bytes());
        b.extend_from_slice(&start.to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        self.buf
    }
}

/// A file or folder name that is safe on Windows, macOS and inside a ZIP.
pub fn safe_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let t = t.trim().trim_matches('.').to_string();
    let t: String = t.chars().take(80).collect();
    if t.is_empty() { "ficheiro".into() } else { t }
}

// ───────── CSV ─────────

/// Semicolon-separated rows with a UTF-8 BOM, so Excel in Portugal opens them with accents and columns intact.
pub fn csv(header: &[&str], rows: &[Vec<String>]) -> Vec<u8> {
    let cell = |s: &str| {
        // A leading = + - @ would make a spreadsheet run it as a formula.
        let s = if s.starts_with(['=', '+', '@']) || (s.starts_with('-') && !s[1..].chars().all(|c| c.is_ascii_digit() || c == ',')) { format!("'{s}") } else { s.to_string() };
        if s.contains([';', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
    };
    let mut out = String::from("\u{feff}");
    out.push_str(&header.iter().map(|h| cell(h)).collect::<Vec<_>>().join(";"));
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&r.iter().map(|c| cell(c)).collect::<Vec<_>>().join(";"));
        out.push_str("\r\n");
    }
    out.into_bytes()
}

/// 12345 cents → "123,45" (Portuguese decimal comma, no thousands separator: safe in CSV).
pub fn cents(c: i64) -> String {
    let sign = if c < 0 { "-" } else { "" };
    let c = c.abs();
    format!("{sign}{},{:02}", c / 100, c % 100)
}

/// 12345 cents → "123,45 €" with thousands separated by a space, for the PDF.
pub fn euros(c: i64) -> String {
    let sign = if c < 0 { "−" } else { "" };
    let c = c.abs();
    let int = (c / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(' ');
        }
        grouped.push(ch);
    }
    format!("{sign}{grouped},{:02} €", c % 100)
}

// ───────── PDF ─────────

/// One line of the summary PDF.
pub enum Line {
    Title(String),
    Heading(String),
    Text(String),
    /// label, amount (right-aligned)
    Row(String, String),
    Gap,
    /// A picture (e.g. a signature), drawn at the given width in points, keeping its proportions.
    Image(Picture, f32),
}

/// A picture ready for the PDF: either 8-bit RGB on white, deflated (from a PNG), or a JPEG passed through as is.
pub struct Picture { w: u32, h: u32, deflated: Vec<u8>, filter: &'static str, colors: &'static str }

impl Picture {
    /// Width over height.
    pub fn aspect(&self) -> f32 {
        self.w as f32 / self.h as f32
    }
}

/// Reads a baseline or progressive JPEG's size and colour model; the bytes go into the PDF unchanged (DCTDecode).
/// CMYK JPEGs (rare, from some scanners) and anything unreadable return None.
pub fn picture_from_jpeg(bytes: &[u8]) -> Option<Picture> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += if marker == 0xFF { 1 } else { 2 };
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        // SOF0–SOF15 except DHT (C4), JPG (C8) and DAC (CC)
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            let seg = bytes.get(i + 4..i + 2 + len)?;
            let (h, w, comps) = (u16::from_be_bytes([seg[1], seg[2]]) as u32, u16::from_be_bytes([seg[3], seg[4]]) as u32, *seg.get(5)?);
            let colors = match comps { 1 => "/DeviceGray", 3 => "/DeviceRGB", _ => return None };
            if w == 0 || h == 0 || w > 12000 || h > 12000 {
                return None;
            }
            return Some(Picture { w, h, deflated: bytes.to_vec(), filter: "/DCTDecode", colors });
        }
        i += 2 + len;
    }
    None
}

/// Decodes a PNG (any colour type, with or without transparency) onto a white background.
/// Returns None when the bytes aren't a PNG this can read, or the picture is absurdly large.
pub fn picture_from_png(bytes: &[u8]) -> Option<Picture> {
    use std::io::Write;
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w > 4000 || h > 4000 {
        return None;
    }
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).ok()?;
    let data = &buf[..frame.buffer_size()];
    let over_white = |c: u8, a: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    match frame.color_type {
        png::ColorType::Rgba => data.chunks_exact(4).for_each(|p| rgb.extend([over_white(p[0], p[3]), over_white(p[1], p[3]), over_white(p[2], p[3])])),
        png::ColorType::Rgb => rgb.extend_from_slice(data),
        png::ColorType::GrayscaleAlpha => data.chunks_exact(2).for_each(|p| { let g = over_white(p[0], p[1]); rgb.extend([g, g, g]) }),
        png::ColorType::Grayscale => data.iter().for_each(|&g| rgb.extend([g, g, g])),
        png::ColorType::Indexed => return None, // EXPAND turns palettes into RGB(A); never reached
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&rgb).ok()?;
    Some(Picture { w, h, deflated: z.finish().ok()?, filter: "/FlateDecode", colors: "/DeviceRGB" })
}

fn win1252(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| match c {
            '€' => 0x80,
            '’' | '‘' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '–' => 0x96,
            '—' => 0x97,
            '−' => b'-',
            '•' => 0x95,
            c if (c as u32) < 0x80 || ((c as u32) >= 0xA0 && (c as u32) <= 0xFF) => c as u32 as u8,
            _ => b'?',
        })
        .collect()
}

fn pdf_text(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for b in win1252(s) {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

/// Approximate Helvetica width (in 1/1000 em) so amounts can be right-aligned.
fn width(s: &str, size: f32) -> f32 {
    let w: f32 = s.chars().map(|c| match c {
        'i' | 'j' | 'l' | '\'' | '.' | ',' | ':' | ';' | '|' => 250.0,
        'f' | 't' | 'r' | 'I' | ' ' | '(' | ')' | '-' => 320.0,
        '0'..='9' | '€' => 556.0,
        'm' | 'w' | 'M' | 'W' => 830.0,
        c if c.is_uppercase() => 667.0,
        _ => 520.0,
    }).sum();
    w * size / 1000.0
}

/// Splits a paragraph into lines that fit `max` points at the given font size (Helvetica, approximate).
pub fn wrap(s: &str, size: f32, max: f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if width(&next, size) <= max || cur.is_empty() {
            cur = next;
            // A single word wider than the line is cut.
            while width(&cur, size) > max && cur.chars().count() > 1 {
                let mut head: String = String::new();
                for c in cur.chars() {
                    if width(&format!("{head}{c}"), size) > max { break; }
                    head.push(c);
                }
                if head.is_empty() { break; }
                cur = cur[head.len()..].to_string();
                out.push(head);
            }
        } else {
            out.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The document queries add the clinic's details as `'clinic_info', (SELECT json_build_object('name', c.name, 'nif', c.nif,
/// 'ers', c.ers_number, 'address', c.address, 'phone', c.phone, 'email', c.email, 'logo', encode(c.logo, 'base64')) FROM clinic c
/// WHERE c.id = 1)`. The logo travels as base64 (PostgreSQL wraps it; the line breaks are ignored here).
/// The top of every document: the logo (if any), the clinic's name, then NIF and ERS registration,
/// address, phone and email — only what's filled in (audit C5: insurers and ADSE need these).
pub fn clinic_header(info: &serde_json::Value, fallback_name: &str) -> Vec<Line> {
    use base64::Engine;
    let s = |k: &str| info[k].as_str().unwrap_or("").trim().to_string();
    let mut out = Vec::new();
    if let Some(pic) = info["logo"].as_str()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b.replace(['\n', '\r'], "")).ok())
        .and_then(|bytes| picture_from_jpeg(&bytes))
    {
        // At most 140 pt wide and 48 pt tall.
        let w = (48.0 * pic.aspect()).min(140.0);
        out.push(Line::Image(pic, w));
    }
    let name = if s("name").is_empty() { fallback_name.trim().to_string() } else { s("name") };
    if !name.is_empty() {
        out.push(Line::Text(name));
    }
    let ids: Vec<String> = [("NIF", s("nif")), ("ERS", s("ers"))].into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k} {v}")).collect();
    if !ids.is_empty() {
        out.push(Line::Text(ids.join(" · ")));
    }
    let address = s("address").split(['\n', '\r']).map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
    if !address.is_empty() {
        for l in wrap(&address, 10.0, 483.0) {
            out.push(Line::Text(l));
        }
    }
    let contacts: Vec<String> = [s("phone"), s("email")].into_iter().filter(|v| !v.is_empty()).collect();
    if !contacts.is_empty() {
        out.push(Line::Text(contacts.join(" · ")));
    }
    out
}

/// An A4 PDF with the given lines (Helvetica, page breaks as needed).
pub fn pdf(lines: &[Line]) -> Vec<u8> {
    let (left, right, top, bottom) = (56.0f32, 539.0f32, 790.0f32, 60.0f32);
    let mut pages: Vec<Vec<u8>> = vec![Vec::new()];
    let mut images: Vec<&Picture> = Vec::new();
    let mut y = top;
    let put = |pages: &mut Vec<Vec<u8>>, y: &mut f32, h: f32, f: &dyn Fn(&mut Vec<u8>, f32)| {
        if *y - h < bottom {
            pages.push(Vec::new());
            *y = top;
        }
        *y -= h;
        f(pages.last_mut().unwrap(), *y);
    };
    let text = |out: &mut Vec<u8>, font: &str, size: f32, x: f32, y: f32, s: &str| {
        out.extend_from_slice(format!("BT /{font} {size} Tf {x:.1} {y:.1} Td ").as_bytes());
        out.extend_from_slice(&pdf_text(s));
        out.extend_from_slice(b" Tj ET\n");
    };
    for l in lines {
        match l {
            Line::Title(s) => put(&mut pages, &mut y, 26.0, &|o, y| text(o, "F2", 18.0, left, y, s)),
            Line::Heading(s) => put(&mut pages, &mut y, 24.0, &|o, y| text(o, "F2", 12.0, left, y, s)),
            Line::Text(s) => put(&mut pages, &mut y, 15.0, &|o, y| text(o, "F1", 10.0, left, y, s)),
            Line::Row(a, b) => put(&mut pages, &mut y, 15.0, &|o, y| {
                text(o, "F1", 10.0, left, y, a);
                text(o, "F1", 10.0, right - width(b, 10.0), y, b);
            }),
            Line::Gap => y -= 8.0,
            Line::Image(p, draw_w) => {
                let draw_h = draw_w * p.h as f32 / p.w as f32;
                let name = format!("Im{}", images.len() + 1);
                images.push(p);
                put(&mut pages, &mut y, draw_h + 6.0, &|o, y| {
                    o.extend_from_slice(format!("q {draw_w:.1} 0 0 {draw_h:.1} {left:.1} {:.1} cm /{name} Do Q\n", y + 3.0).as_bytes());
                });
            }
        }
    }

    // objects: 1 catalog, 2 pages, 3 F1, 4 F2, then the images, then (page, content) pairs
    let mut objs: Vec<Vec<u8>> = Vec::new();
    let n = pages.len();
    let first_page = 5 + images.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", first_page + i * 2)).collect();
    objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objs.push(format!("<< /Type /Pages /Count {n} /Kids [{}] >>", kids.join(" ")).into_bytes());
    objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
    objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec());
    for p in &images {
        let mut o = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} /BitsPerComponent 8 /Filter {} /Length {} >>\nstream\n",
                            p.w, p.h, p.colors, p.filter, p.deflated.len()).into_bytes();
        o.extend_from_slice(&p.deflated);
        o.extend_from_slice(b"\nendstream");
        objs.push(o);
    }
    let xobjects: String = (0..images.len()).map(|i| format!("/Im{} {} 0 R ", i + 1, 5 + i)).collect();
    let xobjects = if xobjects.is_empty() { String::new() } else { format!(" /XObject << {xobjects}>>") };
    for (i, content) in pages.iter().enumerate() {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R /F2 4 0 R >>{xobjects} >> /Contents {} 0 R >>", first_page + 1 + i * 2).into_bytes());
        let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        s.extend_from_slice(content);
        s.extend_from_slice(b"\nendstream");
        objs.push(s);
    }
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

// ───────── joining PDFs ─────────

/// Can this PDF be joined into another? (Readable, and not still encrypted after opening with no password.)
pub fn joinable(bytes: &[u8]) -> bool {
    lopdf::Document::load_mem(bytes).is_ok_and(|d| !d.is_encrypted() && !d.get_pages().is_empty())
}

/// Joins PDFs into one, in order: every page of the first, then every page of the next, and so on.
/// Attributes a page inherits from its old page tree (resources, page size, rotation) are copied onto
/// the page so it looks the same in its new home. Returns None if any part can't be read; check
/// with [`joinable`] first.
pub fn join(parts: &[Vec<u8>]) -> Option<Vec<u8>> {
    use lopdf::{Document, Object, ObjectId};
    let mut out = Document::load_mem(parts.first()?).ok()?;
    let root: ObjectId = out.catalog().ok()?.get(b"Pages").ok()?.as_reference().ok()?;
    let mut kids: Vec<ObjectId> = out.get_pages().into_values().collect();
    for bytes in &parts[1..] {
        let mut d = Document::load_mem(bytes).ok()?;
        if d.is_encrypted() {
            return None;
        }
        d.renumber_objects_with(out.max_id + 1);
        let pages: Vec<ObjectId> = d.get_pages().into_values().collect();
        for &pid in &pages {
            // Collect what the page inherits, walking up its parents.
            let mut inherited: Vec<(&[u8], Object)> = Vec::new();
            let mut parent = d.get_dictionary(pid).ok()?.get(b"Parent").and_then(Object::as_reference).ok();
            let mut guard = 0;
            while let Some(id) = parent {
                guard += 1;
                if guard > 64 {
                    break;
                }
                let Ok(node) = d.get_dictionary(id) else { break };
                for key in [&b"Resources"[..], b"MediaBox", b"CropBox", b"Rotate"] {
                    if inherited.iter().all(|(k, _)| *k != key) {
                        if let Ok(v) = node.get(key) {
                            inherited.push((key, v.clone()));
                        }
                    }
                }
                parent = node.get(b"Parent").and_then(Object::as_reference).ok();
            }
            let page = d.get_object_mut(pid).ok()?.as_dict_mut().ok()?;
            for (key, v) in inherited {
                if !page.has(key) {
                    page.set(key.to_vec(), v);
                }
            }
            if !page.has(b"MediaBox") {
                page.set("MediaBox", vec![0.into(), 0.into(), 595.into(), 842.into()]);
            }
            page.set("Parent", Object::Reference(root));
        }
        out.max_id = out.max_id.max(d.max_id);
        out.objects.extend(d.objects);
        kids.extend(pages);
    }
    let node = out.get_object_mut(root).ok()?.as_dict_mut().ok()?;
    node.set("Kids", kids.iter().map(|&k| Object::Reference(k)).collect::<Vec<_>>());
    node.set("Count", kids.len() as i64);
    // The other files' old catalogues and page trees are no longer referenced: drop them.
    out.prune_objects();
    let mut buf = Vec::new();
    out.save_to(&mut buf).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrap_fits() {
        let long = "Exercício terapêutico com progressão de carga ".repeat(8);
        let lines = wrap(&long, 10.0, 200.0);
        assert!(lines.len() > 3);
        assert!(lines.iter().all(|l| width(l, 10.0) <= 200.0));
        assert_eq!(lines.join(" "), long.trim());
        assert_eq!(wrap("", 10.0, 200.0).len(), 0);
        assert!(wrap(&"x".repeat(200), 10.0, 100.0).iter().all(|l| width(l, 10.0) <= 100.0));
    }
    #[test]
    fn png_signature_in_pdf() {
        // 2×1 RGBA PNG: one opaque black pixel, one transparent pixel (becomes white).
        let mut png_bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png_bytes, 2, 1);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&[0, 0, 0, 255, 0, 0, 0, 0]).unwrap();
        }
        let p = picture_from_png(&png_bytes).expect("decodes");
        let mut raw = Vec::new();
        use std::io::Read;
        flate2::read::ZlibDecoder::new(&p.deflated[..]).read_to_end(&mut raw).unwrap();
        assert_eq!(raw, vec![0, 0, 0, 255, 255, 255]);
        let doc = pdf(&[Line::Title("x".into()), Line::Image(p, 200.0)]);
        let txt = String::from_utf8_lossy(&doc);
        assert!(txt.contains("/Subtype /Image /Width 2 /Height 1"));
        assert!(txt.contains("/XObject << /Im1 5 0 R >>"));
        assert!(txt.contains("/Im1 Do"));
        assert!(picture_from_png(b"not a png").is_none());
    }
    #[test]
    fn jpeg_and_join() {
        // Smallest useful JPEG header: SOI, SOF0 (8-bit, 3×2, 3 components), EOI.
        let jpg = [0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x02, 0x00, 0x03, 0x03, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xFF, 0xD9];
        let p = picture_from_jpeg(&jpg).expect("reads the size");
        assert_eq!((p.w, p.h, p.colors, p.filter), (3, 2, "/DeviceRGB", "/DCTDecode"));
        assert!(picture_from_jpeg(b"not a jpeg").is_none());

        let long: Vec<Line> = (0..80).map(|i| Line::Text(format!("linha {i}"))).collect();
        let a = pdf(&long);
        let b = pdf(&[Line::Title("Fatura".into())]);
        assert!(joinable(&a) && joinable(&b) && !joinable(b"%PDF-1.4 broken"));
        let joined = join(&[a, b]).expect("joins");
        let d = lopdf::Document::load_mem(&joined).unwrap();
        assert_eq!(d.get_pages().len(), 3);
        for (_, id) in d.get_pages() {
            let page = d.get_dictionary(id).unwrap();
            assert!(page.has(b"Resources") && page.has(b"MediaBox"));
        }
    }
    #[test]
    fn crc_known() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
    #[test]
    fn money_format() {
        assert_eq!(cents(12345), "123,45");
        assert_eq!(cents(-5), "-0,05");
        assert_eq!(euros(123456789), "1 234 567,89 €");
    }
    #[test]
    fn csv_quotes_and_formulas() {
        let s = String::from_utf8(csv(&["a"], &[vec!["x;y".into()], vec!["=1+1".into()], vec!["-12,50".into()]])).unwrap();
        assert!(s.contains("\"x;y\"") && s.contains("'=1+1") && s.contains("\r\n-12,50"));
    }
}
