# Outside reference points and journal-page reading views (2026-11-18)

## PDF conversion on the 200 PDFs: pdftotext, GROBID and Lunta

| | pdftotext | pdftotext -layout | GROBID 0.9.1 (CRF) | Lunta Swift | Lunta Rust | XML view (control) |
|---|---|---|---|---|---|---|
| Token F1, median (IQR) | 0.716 (0.658–0.767) | 0.491 (0.448–0.565) | 0.947 (0.895–0.974) | 0.955 (0.918–0.980) | 0.960 (0.912–0.982) | 1.000 (0.999–1.000) |
| Token F1, mean [CI] | 0.702 | 0.523 | 0.926 | 0.927 | 0.932 | 0.999 |
| Recall, median | 0.975 | 0.643 | 0.985 | 0.988 | 0.993 | 1.000 |
| Precision, median | 0.570 | 0.397 | 0.930 | 0.940 | 0.939 | 0.999 |
| Kendall's τ, median | 0.992 | 0.937 | 0.998 | 1.000 | 1.000 | 1.000 |
| Broken words / 1000, mean | 0.87 | 2.45 | 0.43 | 0.79 | 0.46 | 0.14 |
| Papers with interleaving, % | 60.5 | 85.5 | 14.5 | 20.5 | 12.5 | 1.0 |
| Papers with any boilerplate leak, % | 100.0 | 100.0 | 47.0 | 41.0 | 41.5 | 0.5 |
| Figures identified (image + label), % | 0.0 | 0.0 | 0.0 | 66.4 | 72.2 | 0.0 |
| Reference items / XML refs, median | 0.00 | 0.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| Papers with no reference items | 199 | 199 | 0 | 2 | 2 | 2 |
| s per page, median | 0.005 |  | 0.173 |  |  |  |

swift − GROBID token F1: mean +0.002 [-0.011, +0.014], Wilcoxon p = 1.3e-01; swift better in 107, worse in 93

rust − GROBID token F1: mean +0.006 [-0.005, +0.017], Wilcoxon p = 6.1e-02; rust better in 110, worse in 90

swift_found_reference_list (n = 197): median F1 swift 0.956, rust 0.961, grobid 0.948, pdftotext 0.715

swift_no_reference_items (n = 3): median F1 swift 0.614, rust 0.624, grobid 0.831, pdftotext 0.749

## Reading views from the journal's article page

Attempted 92, saved 37, converted 30, refused as abstract-only 7.
Blocked by host: [('link.springer.com', 10), ('mdpi.com', 6), ('academic.oup.com', 3), ('hindawi.com', 3), ('doi.org', 3), ('journals.asm.org', 3), ('pubs.acs.org', 2), ('onlinelibrary.wiley.com', 2), ('advanced.onlinelibrary.wiley.com', 1), ('aging-us.com', 1), ('journals.lww.com', 1), ('journals.iucr.org', 1), ('jbc.org', 1), ('cureus.com', 1), ('pubs.rsc.org', 1), ('ijccm.org', 1), ('thieme-connect.de', 1), ('ovid.com', 1), ('jmg.bmj.com', 1), ('jmir.org', 1), ('tandfonline.com', 1), ('oncotarget.com', 1), ('thelancet.com', 1), ('medicinaoral.com', 1), ('mental.jmir.org', 1), ('peerj.com', 1), ('journals.sagepub.com', 1), ('elifesciences.org', 1), ('cell.com', 1), ('researchprotocols.org', 1), ('sciencedirect.com', 1)]
Reasons: [('HTTP 403', 34), ('robots.txt', 13), ('network error', 3), ('HTTP 202', 3), ('not an HTML page (the DOI leads to a PDF)', 1), ('bot check', 1)]
Readers: [('generic', 25), ('Springer Nature', 5)]
Families: [('Frontiers', 12), ('other', 7), ('PLOS', 6), ('Nature', 5)]

| same papers | page view (Swift) | page view (Rust) | PDF view (Swift) | PDF view (Rust) | XML view | GROBID | pdftotext |
|---|---|---|---|---|---|---|---|
| n | 30 | 30 | 30 | 30 | 30 | 30 | 30 |
| Token F1, median (IQR) | 0.956 (0.943–0.968) | 0.956 (0.943–0.968) | 0.909 (0.833–0.957) | 0.934 (0.866–0.969) | 1.000 (0.998–1.000) | 0.938 (0.889–0.964) | 0.628 (0.545–0.675) |
| Recall, median | 0.995 | 0.995 | 0.973 | 0.978 | 1.000 | 0.967 | 0.903 |
| Precision, median | 0.924 | 0.924 | 0.862 | 0.909 | 0.999 | 0.928 | 0.484 |
| Broken words / 1000, mean | 0.05 | 0.05 | 0.55 | 0.46 | 0.01 | 0.74 | 0.64 |
| Papers with interleaving, % | 0.0 | 0.0 | 26.7 | 10.0 | 0.0 | 16.7 | 66.7 |
| Papers with any boilerplate leak, % | 76.7 | 76.7 | 46.7 | 43.3 | 0.0 | 43.3 | 100.0 |
| Figures identified, % | 82.4 | 82.4 | 84.6 | 85.3 | 0.0 | 0.0 | 0.0 |
| Tables identified, % | 8.4 | 8.4 | 53.0 | 50.6 | 100.0 | 0.0 | 0.0 |
| Reference items / XML refs, median | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0.00 |

page − PDF view (Swift) token F1: mean +0.048 [+0.012, +0.082], p = 4.0e-03; better in 23, worse in 7

| family | n | page F1 median | PDF F1 median | page figures % | PDF figures % | page refs ratio |
|---|---|---|---|---|---|---|
| Frontiers | 12 | 0.956 | 0.844 | 100.0 | 95.3 | 1.00 |
| PLOS | 6 | 0.938 | 0.909 | 100.0 | 96.9 | 1.00 |
| other | 7 | 0.964 | 0.926 | 85.0 | 65.0 | 1.05 |
| Nature | 5 | 0.986 | 0.918 | 48.8 | 73.2 | 1.00 |

## Highlights

| from → to | n | exact | spacing | folded | ends | lost | correct [CI] | wrong place | guard pass k=1/3/5/10/20 |
|---|---|---|---|---|---|---|---|---|---|
| swift->page-swift | 600 (30 papers) | 69.5 | 8.5 | 6.3 | 4.8 | 10.8 | 88.0 [84.5, 91.2] | 1.2 | 0.89 / 0.72 / 0.58 / 0.37 / 0.17 |
| rust->page-rust | 600 (30 papers) | 73.5 | 8.0 | 5.7 | 2.3 | 10.5 | 88.5 [84.3, 92.3] | 1.0 | 0.90 / 0.74 / 0.62 / 0.45 / 0.30 |
| swift->xml | 600 (30 papers) | 64.0 | 9.8 | 8.3 | 4.8 | 13.0 | 86.2 [81.7, 90.2] | 0.8 | 0.87 / 0.68 / 0.56 / 0.38 / 0.20 |
| xml->page-swift | 600 (30 papers) | 86.2 | 8.2 | 4.2 | 0.5 | 1.0 | 98.8 [97.7, 99.7] | 0.2 | 0.99 / 0.97 / 0.95 / 0.91 / 0.83 |
| swift->xml (all 199) | 4000 (200 papers) | 73.0 | 6.5 | 8.5 | 2.7 | 9.2 | 89.5 [87.8, 91.0] | 1.3 | 0.91 / 0.77 / 0.66 / 0.48 / 0.28 |
