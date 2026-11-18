//! The citation card in the PDF view (PDF.js, desktop/src/pdfcite.ts): the reference a clicked
//! citation points to, read from the PDF's lines with PDFium and found by the core
//! (lunta_core::refcite, as PDFCitations.swift does with PDFKit on the Mac), and its DOI.

use crate::{err, AppState, R};
use lunta_core::model::FileKind;
use lunta_core::refcite::{self, TextLine};
use lunta_core::Library;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

/// One PDF's lines and each page's media box origin (left, top) in PDF user space.
type Lines = Arc<(Vec<TextLine>, Vec<(f64, f64)>)>;

/// The lines of the last PDF asked about (the one open in the reader).
static CACHE: Mutex<Option<(String, Lines)>> = Mutex::new(None);

/// Reads the PDF (blocking: call from spawn_blocking).
fn lines(lib: &Library, id: &str) -> R<Lines> {
    if let Some((cid, l)) = CACHE.lock().unwrap().as_ref() {
        if cid == id {
            return Ok(l.clone());
        }
    }
    let path = lib.file_path(id.to_string(), FileKind::Pdf).ok_or("This paper has no PDF on this computer.")?;
    let data = std::fs::read(path).map_err(err)?;
    let l: Lines = Arc::new(lunta_core::convert::pdf::text_lines(&data).map_err(err)?);
    *CACHE.lock().unwrap() = Some((id.to_string(), l.clone()));
    Ok(l)
}

/// Reads the PDF's lines ahead of the first click (when the PDF view opens).
#[tauri::command]
pub async fn pdf_prepare_citations(state: State<'_, AppState>, id: String) -> R<()> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lines(&lib, &id).map(|_| ())).await.map_err(err)?
}

/// The reference at a link's destination: `page` from 0, `x` / `y` in PDF user space (as PDF.js
/// gives them). None when the link goes somewhere else (a section, a figure).
#[tauri::command]
pub async fn pdf_reference_at(state: State<'_, AppState>, id: String, page: u32, x: Option<f64>, y: f64) -> R<Option<String>> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || {
        let l = lines(&lib, &id)?;
        let (left, top) = l.1.get(page as usize).copied().unwrap_or((0.0, 0.0));
        Ok(refcite::reference_at(&l.0, page, x.map(|x| x - left), top - y))
    })
    .await
    .map_err(err)?
}

/// The reference for a citation without a link ("[12]", a superscript number, "(Smith et al.,
/// 2020)") at `offset` (UTF-16) in a line of text; None when there is none there.
#[tauri::command]
pub async fn pdf_reference_for_citation(state: State<'_, AppState>, id: String, text: String, offset: u32) -> R<Option<String>> {
    if !refcite::is_citation_at(&text, offset as usize) {
        return Ok(None);
    }
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || Ok(refcite::reference_for_citation(&lines(&lib, &id)?.0, &text, offset as usize)))
        .await
        .map_err(err)?
}

#[derive(Serialize)]
pub struct Lookup {
    doi: String,
    arxiv: String,
    /// the paper, when it is in the library
    in_library: Option<String>,
}

/// A reference's DOI or arXiv number (printed in it, else Crossref's match) and whether that paper
/// is in the library. None when not found.
#[tauri::command]
pub async fn lookup_reference(state: State<'_, AppState>, text: String) -> R<Option<Lookup>> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(ids) = refcite::resolve_reference(&text).map_err(err)? else { return Ok(None) };
        let in_library = crate::paper_in_library(&lib, &ids.doi, &ids.arxiv);
        Ok(Some(Lookup { doi: ids.doi, arxiv: ids.arxiv, in_library }))
    })
    .await
    .map_err(err)?
}
