## Corpus

Main corpus: 200 articles, 200 journals, 52 publishers, 2004–2026; pages median 11 (IQR 8–14), 2393 pages in total; body words median 4512.

| Year stratum | n |
|---|---|
| 2010-2014 | 40 |
| 2015-2018 | 40 |
| 2019-2021 | 40 |
| 2022-2026 | 40 |
| <=2009 | 40 |

| Publisher | n |
|---|---|
| BMC | 54 |
| Multidisciplinary Digital Publishing Institute (MDPI) | 26 |
| Wiley | 22 |
| (none) | 15 |
| Frontiers Media SA | 12 |
| PLOS | 6 |
| Nature Publishing Group | 5 |
| Oxford University Press | 4 |
| JMIR Publications Inc. | 3 |
| Springer | 3 |
| American Society for Microbiology (ASM) | 3 |
| Elsevier | 3 |
| SAGE Publications | 2 |
| American Chemical Society | 2 |
| Impact Journals, LLC | 2 |
| 37 other publishers | 38 |

| Article type | n |
|---|---|
| research-article | 174 |
| review-article | 17 |
| other | 4 |
| brief-report | 3 |
| data-paper | 1 |
| case-report | 1 |

Author-manuscript subset: 30 NIHMS manuscripts, 30 journals, 2011–2026, pages median 21.


## Conversion success (main corpus)

| System | EPUB produced |
|---|---|
| Swift (current) | 200/200 |
| Swift (baseline 38d2bdc^) | 200/200 |
| Rust (PDFium) | 200/200 |
| XML reading view (control) | 200/200 |

Paired analysis set (all three PDF converters succeeded): n = 200.

## Text metrics (per paper: median (IQR); mean [95% bootstrap CI])

| Metric | Swift (current) | Swift (baseline 38d2bdc^) | Rust (PDFium) | XML reading view (control) |
|---|---|---|---|---|
| Token F1 (LCS, vs XML) | 0.957 (0.921–0.981); mean 0.930 [0.917, 0.943] | 0.890 (0.798–0.948); mean 0.860 [0.841, 0.877] | 0.960 (0.912–0.982); mean 0.933 [0.919, 0.944] | 1.000 (0.999–1.000); mean 0.999 [0.998, 0.999] |
| Token precision | 0.942 (0.884–0.974); mean 0.910 [0.895, 0.923] | 0.861 (0.728–0.945); mean 0.821 [0.801, 0.841] | 0.939 (0.886–0.972); mean 0.906 [0.890, 0.920] | 0.999 (0.998–1.000); mean 0.997 [0.997, 0.998] |
| Token recall (abstract+body) | 0.989 (0.967–0.996); mean 0.960 [0.947, 0.971] | 0.966 (0.906–0.986); mean 0.924 [0.907, 0.940] | 0.994 (0.976–0.997); mean 0.972 [0.960, 0.981] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Word error rate | 0.083 (0.042–0.165); mean 0.158 [0.119, 0.219] | 0.221 (0.106–0.415); mean 0.312 [0.264, 0.374] | 0.080 (0.041–0.171); mean 0.160 [0.120, 0.220] | 0.001 (0.000–0.002); mean 0.003 [0.002, 0.004] |
| Bag-of-words F1 (order-free) | 0.969 (0.941–0.985); mean 0.948 [0.936, 0.957] | 0.925 (0.845–0.959); mean 0.887 [0.870, 0.903] | 0.969 (0.928–0.985); mean 0.941 [0.927, 0.952] | 1.000 (0.999–1.000); mean 0.999 [0.998, 0.999] |
| Kendall's tau (paragraph order) | 1.000 (0.997–1.000); mean 0.986 [0.979, 0.993] | 1.000 (0.995–1.000); mean 0.982 [0.973, 0.989] | 1.000 (1.000–1.000); mean 0.989 [0.982, 0.995] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Adjacent paragraph pairs in order | 1.000 (0.983–1.000); mean 0.989 [0.986, 0.991] | 1.000 (0.977–1.000); mean 0.986 [0.983, 0.989] | 1.000 (1.000–1.000); mean 0.992 [0.990, 0.994] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Interleaving events / paper | 0.000 (0.000–0.000); mean 2.270 [0.955, 3.860] | 0.000 (0.000–1.250); mean 3.375 [2.065, 4.965] | 0.000 (0.000–0.000); mean 0.210 [0.115, 0.315] | 0.000 (0.000–0.000); mean 0.025 [0.000, 0.070] |
| Broken words / 1000 tokens | 0.000 (0.000–0.326); mean 0.517 [0.312, 0.778] | 0.269 (0.000–1.086); mean 4.150 [2.647, 5.927] | 0.000 (0.000–0.276); mean 0.456 [0.250, 0.707] | 0.000 (0.000–0.000); mean 0.135 [0.067, 0.231] |
|   spurious splits / 1k | 0.000 (0.000–0.000); mean 0.248 [0.062, 0.502] | 0.000 (0.000–0.585); mean 3.865 [2.379, 5.648] | 0.000 (0.000–0.000); mean 0.257 [0.071, 0.499] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.001] |
|   spurious joins / 1k | 0.000 (0.000–0.186); mean 0.177 [0.123, 0.241] | 0.000 (0.000–0.165); mean 0.233 [0.130, 0.379] | 0.000 (0.000–0.000); mean 0.141 [0.072, 0.241] | 0.000 (0.000–0.000); mean 0.135 [0.066, 0.233] |
|   leftover line-break hyphens / 1k | 0.000 (0.000–0.000); mean 0.092 [0.037, 0.168] | 0.000 (0.000–0.000); mean 0.051 [0.026, 0.086] | 0.000 (0.000–0.000); mean 0.059 [0.028, 0.099] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
| Lost compound hyphens / 1000 tokens | 0.579 (0.190–1.148); mean 0.790 [0.685, 0.901] | 0.584 (0.253–1.208); mean 0.809 [0.704, 0.919] | 0.582 (0.200–1.173); mean 0.793 [0.697, 0.897] | 0.000 (0.000–0.000); mean 0.002 [0.000, 0.005] |
| Boilerplate leaks / paper | 0.000 (0.000–1.000); mean 0.810 [0.515, 1.195] | 1.000 (0.000–2.000); mean 1.625 [1.235, 2.080] | 0.000 (0.000–1.000); mean 1.030 [0.605, 1.605] | 0.000 (0.000–0.000); mean 0.115 [0.000, 0.345] |
|   running headers/footers | 0.000 (0.000–0.000); mean 0.130 [0.060, 0.225] | 0.000 (0.000–0.000); mean 0.330 [0.200, 0.485] | 0.000 (0.000–0.000); mean 0.215 [0.075, 0.425] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
|   page numbers | 0.000 (0.000–0.000); mean 0.335 [0.070, 0.715] | 0.000 (0.000–0.000); mean 0.425 [0.160, 0.795] | 0.000 (0.000–0.000); mean 0.470 [0.115, 1.050] | 0.000 (0.000–0.000); mean 0.115 [0.000, 0.345] |
|   publisher/licence lines | 0.000 (0.000–1.000); mean 0.345 [0.255, 0.450] | 1.000 (0.000–1.000); mean 0.870 [0.715, 1.030] | 0.000 (0.000–1.000); mean 0.345 [0.255, 0.450] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
| Papers with any boilerplate leak (%) | 40.0 [33.0, 47.0] | 63.5 [56.5, 70.0] | 42.0 [35.0, 49.0] | 0.5 [0.0, 1.5] |
| Papers with any interleaving (%) | 20.5 [15.0, 26.0] | 34.0 [27.5, 40.5] | 11.5 [7.5, 16.5] | 1.0 [0.0, 2.5] |
| Papers with token F1 < 0.9 (%) | 19.5 [14.0, 25.0] | 53.5 [46.5, 60.5] | 21.5 [16.0, 27.5] | 0.0 [0.0, 0.0] |

## Current vs baseline (paired, current − baseline; and Rust − Swift)

| Metric | Δ mean [95% CI] (Swift current − baseline) | Wilcoxon p | better/worse/same | Δ mean [95% CI] (Rust − Swift) |
|---|---|---|---|---|
| Token F1 (LCS, vs XML) | +0.0706 [+0.0595, +0.0826] | 1.9e-29 | 176/15/9 | +0.0022 [-0.0055, +0.0103] |
| Token precision | +0.0887 [+0.0715, +0.1074] | 9.2e-24 | 165/26/9 | -0.0044 [-0.0157, +0.0061] |
| Token recall (abstract+body) | +0.0351 [+0.0284, +0.0430] | 2.8e-25 | 153/12/35 | +0.0120 [+0.0066, +0.0183] |
| Word error rate | -0.1544 [-0.1837, -0.1272] | 2.6e-28 | 174/17/9 | +0.0023 [-0.0143, +0.0195] |
| Bag-of-words F1 (order-free) | +0.0602 [+0.0495, +0.0716] | 4.4e-28 | 172/17/11 | -0.0070 [-0.0131, -0.0017] |
| Kendall's tau (paragraph order) | +0.0047 [+0.0006, +0.0096] | 8.9e-05 | 45/9/145 | +0.0028 [+0.0009, +0.0052] |
| Adjacent paragraph pairs in order | +0.0021 [+0.0002, +0.0041] | 0.0022 | 41/9/149 | +0.0034 [+0.0017, +0.0055] |
| Interleaving events / paper | -1.1050 [-1.6651, -0.5550] | 3.7e-05 | 43/13/144 | -2.0600 [-3.7400, -0.7600] |
| Broken words / 1000 tokens | -3.6336 [-5.3949, -2.1085] | 5.3e-09 | 86/38/76 | -0.0605 [-0.1682, +0.0551] |
|   spurious splits / 1k | -3.6178 [-5.3793, -2.0974] | 8.4e-12 | 67/11/122 | +0.0089 [-0.0250, +0.0429] |
|   spurious joins / 1k | -0.0560 [-0.1870, +0.0183] | 0.67 | 33/37/130 | -0.0367 [-0.1235, +0.0726] |
|   leftover line-break hyphens / 1k | +0.0403 [-0.0116, +0.1032] | 0.023 | 11/21/168 | -0.0327 [-0.0936, -0.0000] |
| Lost compound hyphens / 1000 tokens | -0.0190 [-0.0642, +0.0205] | 0.93 | 77/83/40 | +0.0025 [-0.0594, +0.0488] |
| Boilerplate leaks / paper | -0.8150 [-1.0650, -0.5950] | 1.9e-13 | 76/4/120 | +0.2200 [+0.0200, +0.5050] |
|   running headers/footers | -0.2000 [-0.3050, -0.1100] | 1e-05 | 23/0/177 | +0.0850 [-0.0150, +0.2700] |
|   page numbers | -0.0900 [-0.1650, -0.0250] | 0.0046 | 16/2/182 | +0.1350 [+0.0100, +0.3450] |
|   publisher/licence lines | -0.5250 [-0.6700, -0.3949] | 9.1e-12 | 62/3/135 | +0.0000 [-0.0150, +0.0150] |
| Papers with any leak (share) | -0.2350 [-0.3000, -0.1700] | 1.1e-10 | 50/3/147 | +0.0200 [-0.0050, +0.0500] |

## Figures, tables, equations, references (pooled over papers [95% CI])

| | Swift (current) | Swift (baseline 38d2bdc^) | Rust (PDFium) |
|---|---|---|---|
| Figure images, count-based recall Σmin(out,xml)/Σxml | 96.0 [91.3, 99.0] | 96.2 [91.6, 99.2] | 96.0 [91.4, 99.1] |
| Figure images, count-based precision Σmin(out,xml)/Σout | 72.7 [67.4, 78.2] | 62.5 [57.3, 67.9] | 72.5 [67.1, 78.2] |
| Figure recall, identified (image whose caption/label matches an XML figure) | 66.4 [59.0, 73.4] | 65.7 [58.6, 72.5] | 72.8 [66.2, 79.6] |
| Figure precision (output figures matching an XML figure) | 50.3 [44.1, 56.7] | 42.7 [37.5, 48.1] | 55.0 [48.6, 61.6] |
| Figure captions kept (≥80 % of first 50 words) | 61.9 [54.9, 69.2] | 55.0 [47.8, 62.2] | 60.3 [52.2, 68.0] |
| Table images, count-based recall | 68.0 [61.0, 75.0] | 62.0 [55.1, 68.8] | 68.6 [61.1, 75.6] |
| Table recall, identified (kept as image, label/caption matches) | 65.9 [58.5, 72.9] | 56.0 [49.5, 62.6] | 66.7 [59.2, 73.8] |
| Table precision | 94.1 [90.9, 97.0] | 87.7 [83.7, 91.4] | 94.4 [91.2, 97.4] |
| Table captions kept | 73.0 [66.6, 79.1] | 64.0 [57.3, 70.4] | 73.8 [67.2, 79.7] |
| Display equations kept as images (min(out,xml)/xml) | 93.5 [65.5, 98.3] | 52.0 [23.7, 58.3] | 94.2 [69.7, 98.7] |
| Exact figure count (% papers) | 47.0 | 30.5 | 45.5 |
| Figures: XML total / output total | 849 / 1121 | 849 / 1308 | 849 / 1124 |
| Tables: XML total / output total | 484 / 339 | 484 / 309 | 484 / 342 |
| Display equations: XML total / output images | 277 / 276 | 277 / 218 | 277 / 292 |
| Reference items out/XML (median ratio) | 1.00 | 0.85 | 1.00 |

## Figures cut from the PDF for an XML version without images

| Subset | Papers | Figures recovered (pooled [95% CI]) | Papers with all figures | Papers with none |
|---|---|---|---|---|
| author manuscripts | 30/30 | 96.9 [90.8, 100.0] | 93.3 % | 3.3 % |
| main corpus | 200/200 | 48.8 [40.2, 57.4] | 46.0 % | 55.0 % |

## Highlight survival (20 simulated highlights per paper; % of highlights [95% CI])


Main corpus (paired set)

| Scenario | exact | whitespace | fold | start/end | lost | anchored correctly | anchored to wrong text |
|---|---|---|---|---|---|---|---|
| swift->xml (n=200 papers) | 74.2 | 6.8 | 8.5 | 2.1 | 8.3 | 90.5 [88.8, 92.0] | 1.2 |
| base->xml (n=200 papers) | 66.3 | 5.1 | 9.9 | 3.8 | 14.9 | 82.9 [80.6, 84.9] | 2.3 |
| rust->xml (n=200 papers) | 75.9 | 5.8 | 7.8 | 2.1 | 8.5 | 90.6 [88.8, 92.1] | 0.9 |
| base->swift (n=200 papers) | 88.0 | 0.6 | 4.2 | 1.4 | 5.8 | 93.1 [91.4, 94.6] | 1.1 |

Author manuscripts

| Scenario | exact | whitespace | fold | start/end | lost | anchored correctly | anchored to wrong text |
|---|---|---|---|---|---|---|---|
| swift->xml (n=30 papers) | 90.2 | 4.3 | 1.3 | 1.2 | 3.0 | 96.7 [94.8, 98.2] | 0.3 |
| base->xml (n=30 papers) | 89.7 | 4.3 | 2.2 | 0.8 | 3.0 | 96.0 [94.3, 97.5] | 1.0 |
| rust->xml (n=30 papers) | 91.7 | 4.3 | 0.7 | 0.7 | 2.7 | 96.8 [95.0, 98.5] | 0.5 |
| base->swift (n=30 papers) | 99.0 | 0.2 | 0.8 | 0.0 | 0.0 | 100.0 [100.0, 100.0] | 0.0 |

## Speed

| System | setting | s/page median (IQR) | CPU s/page | s/paper | peak RSS MB |
|---|---|---|---|---|---|
| Swift (current) | isolated (one conversion at a time) | 0.124 (0.105–0.142) | 0.085 (0.077–0.091) | 1.17 | 75 |
| Rust (PDFium) | isolated (one conversion at a time) | 0.047 (0.031–0.065) | 0.046 (0.030–0.064) | 0.42 | 103 |
| Swift (baseline 38d2bdc^) | batch (3 conversions in parallel) | 0.190 (0.146–0.254) | 0.085 (0.074–0.097) | 2.02 | 75 |

Batch run (3 conversions in parallel, all 200 papers): Swift (current) 0.135 s/page wall, 0.098 s/page CPU; Swift (baseline 38d2bdc^) 0.190 s/page wall, 0.085 s/page CPU; Rust (PDFium) 0.046 s/page wall, 0.046 s/page CPU.

## Median token F1 / broken words per 1k by year stratum

| Group | n | F1 Swift current | F1 baseline | F1 Rust | broken/1k current | broken/1k baseline | broken/1k Rust |
|---|---|---|---|---|---|---|---|
| 2010-2014 | 40 | 0.943 | 0.827 | 0.959 | 0.00 | 0.00 | 0.00 |
| 2015-2018 | 40 | 0.970 | 0.898 | 0.972 | 0.06 | 0.28 | 0.00 |
| 2019-2021 | 40 | 0.960 | 0.881 | 0.963 | 0.00 | 0.29 | 0.00 |
| 2022-2026 | 40 | 0.963 | 0.873 | 0.971 | 0.00 | 0.55 | 0.00 |
| <=2009 | 40 | 0.952 | 0.932 | 0.899 | 0.18 | 0.28 | 0.29 |

## Median token F1 / broken words per 1k by publisher (≥ 5 papers)

| Group | n | F1 Swift current | F1 baseline | F1 Rust | broken/1k current | broken/1k baseline | broken/1k Rust |
|---|---|---|---|---|---|---|---|
| BMC | 54 | 0.959 | 0.844 | 0.947 | 0.09 | 0.17 | 0.16 |
| Multidisciplinary Digital Publishing Institute (MDPI) | 26 | 0.984 | 0.858 | 0.986 | 0.00 | 16.34 | 0.00 |
| Wiley | 22 | 0.955 | 0.916 | 0.979 | 0.15 | 0.37 | 0.00 |
| (none) | 15 | 0.958 | 0.923 | 0.970 | 0.17 | 0.17 | 0.00 |
| Frontiers Media SA | 12 | 0.844 | 0.834 | 0.921 | 0.00 | 0.00 | 0.00 |
| PLOS | 6 | 0.911 | 0.893 | 0.912 | 0.05 | 0.05 | 0.00 |
| Nature Publishing Group | 5 | 0.951 | 0.893 | 0.959 | 0.18 | 0.69 | 0.18 |

## Swift vs Rust agreement (token F1 between the two outputs)

- swift-rust: 0.994 [0.989, 0.995] (IQR 0.965–0.999)
- swift-base: 0.953 [0.935, 0.962] (IQR 0.862–0.981)
- base-rust: 0.922 [0.902, 0.934] (IQR 0.840–0.971)
- Pearson r of per-paper F1 vs XML (Swift, Rust): 0.82
