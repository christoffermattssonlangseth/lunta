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
| Token F1 (LCS, vs XML) | 0.955 (0.918–0.980); mean 0.927 [0.914, 0.940] | 0.890 (0.798–0.948); mean 0.860 [0.841, 0.877] | 0.960 (0.912–0.982); mean 0.932 [0.919, 0.944] | 1.000 (0.999–1.000); mean 0.999 [0.998, 0.999] |
| Token precision | 0.940 (0.878–0.972); mean 0.905 [0.890, 0.919] | 0.861 (0.728–0.945); mean 0.821 [0.801, 0.841] | 0.939 (0.878–0.971); mean 0.905 [0.889, 0.919] | 0.999 (0.998–1.000); mean 0.997 [0.997, 0.998] |
| Token recall (abstract+body) | 0.988 (0.967–0.996); mean 0.959 [0.946, 0.971] | 0.966 (0.906–0.986); mean 0.924 [0.907, 0.940] | 0.993 (0.976–0.997); mean 0.972 [0.960, 0.981] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Word error rate | 0.087 (0.043–0.170); mean 0.164 [0.124, 0.226] | 0.221 (0.106–0.415); mean 0.312 [0.264, 0.374] | 0.079 (0.041–0.176); mean 0.161 [0.121, 0.221] | 0.001 (0.000–0.002); mean 0.003 [0.002, 0.004] |
| Bag-of-words F1 (order-free) | 0.969 (0.937–0.984); mean 0.945 [0.932, 0.955] | 0.925 (0.845–0.959); mean 0.887 [0.870, 0.903] | 0.968 (0.928–0.985); mean 0.940 [0.927, 0.951] | 1.000 (0.999–1.000); mean 0.999 [0.998, 0.999] |
| Kendall's tau (paragraph order) | 1.000 (0.997–1.000); mean 0.986 [0.979, 0.992] | 1.000 (0.995–1.000); mean 0.982 [0.973, 0.989] | 1.000 (1.000–1.000); mean 0.989 [0.982, 0.995] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Adjacent paragraph pairs in order | 1.000 (0.983–1.000); mean 0.989 [0.986, 0.992] | 1.000 (0.977–1.000); mean 0.986 [0.983, 0.989] | 1.000 (1.000–1.000); mean 0.992 [0.990, 0.994] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Interleaving events / paper | 0.000 (0.000–0.000); mean 2.265 [0.965, 3.855] | 0.000 (0.000–1.250); mean 3.375 [2.065, 4.965] | 0.000 (0.000–0.000); mean 0.255 [0.140, 0.405] | 0.000 (0.000–0.000); mean 0.025 [0.000, 0.070] |
| Broken words / 1000 tokens | 0.101 (0.000–0.573); mean 0.793 [0.449, 1.278] | 0.269 (0.000–1.086); mean 4.150 [2.647, 5.927] | 0.000 (0.000–0.291); mean 0.460 [0.253, 0.711] | 0.000 (0.000–0.000); mean 0.135 [0.067, 0.231] |
|   spurious splits / 1k | 0.000 (0.000–0.000); mean 0.519 [0.177, 0.986] | 0.000 (0.000–0.585); mean 3.865 [2.379, 5.648] | 0.000 (0.000–0.000); mean 0.257 [0.071, 0.499] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.001] |
|   spurious joins / 1k | 0.000 (0.000–0.186); mean 0.181 [0.125, 0.246] | 0.000 (0.000–0.165); mean 0.233 [0.130, 0.379] | 0.000 (0.000–0.000); mean 0.144 [0.075, 0.245] | 0.000 (0.000–0.000); mean 0.135 [0.066, 0.233] |
|   leftover line-break hyphens / 1k | 0.000 (0.000–0.000); mean 0.093 [0.039, 0.168] | 0.000 (0.000–0.000); mean 0.051 [0.026, 0.086] | 0.000 (0.000–0.000); mean 0.059 [0.028, 0.099] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
| Lost compound hyphens / 1000 tokens | 0.578 (0.190–1.147); mean 0.791 [0.685, 0.902] | 0.584 (0.253–1.208); mean 0.809 [0.704, 0.919] | 0.585 (0.200–1.173); mean 0.792 [0.697, 0.895] | 0.000 (0.000–0.000); mean 0.002 [0.000, 0.005] |
| Boilerplate leaks / paper | 0.000 (0.000–1.000); mean 0.830 [0.540, 1.210] | 1.000 (0.000–2.000); mean 1.625 [1.235, 2.080] | 0.000 (0.000–1.000); mean 1.025 [0.600, 1.600] | 0.000 (0.000–0.000); mean 0.115 [0.000, 0.345] |
|   running headers/footers | 0.000 (0.000–0.000); mean 0.140 [0.065, 0.240] | 0.000 (0.000–0.000); mean 0.330 [0.200, 0.485] | 0.000 (0.000–0.000); mean 0.215 [0.075, 0.425] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
|   page numbers | 0.000 (0.000–0.000); mean 0.335 [0.075, 0.705] | 0.000 (0.000–0.000); mean 0.425 [0.160, 0.795] | 0.000 (0.000–0.000); mean 0.465 [0.110, 1.040] | 0.000 (0.000–0.000); mean 0.115 [0.000, 0.345] |
|   publisher/licence lines | 0.000 (0.000–1.000); mean 0.355 [0.260, 0.460] | 1.000 (0.000–1.000); mean 0.870 [0.715, 1.030] | 0.000 (0.000–1.000); mean 0.345 [0.255, 0.450] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
| Papers with any boilerplate leak (%) | 41.0 [34.0, 48.0] | 63.5 [56.5, 70.0] | 41.5 [35.0, 48.5] | 0.5 [0.0, 1.5] |
| Papers with any interleaving (%) | 20.5 [15.0, 26.0] | 34.0 [27.5, 40.5] | 12.5 [8.0, 17.5] | 1.0 [0.0, 2.5] |
| Papers with token F1 < 0.9 (%) | 20.5 [15.0, 26.0] | 53.5 [46.5, 60.5] | 21.5 [16.0, 27.5] | 0.0 [0.0, 0.0] |

## Current vs baseline (paired, current − baseline; and Rust − Swift)

| Metric | Δ mean [95% CI] (Swift current − baseline) | Wilcoxon p | better/worse/same | Δ mean [95% CI] (Rust − Swift) |
|---|---|---|---|---|
| Token F1 (LCS, vs XML) | +0.0676 [+0.0563, +0.0799] | 3.6e-28 | 171/18/11 | +0.0047 [-0.0029, +0.0128] |
| Token precision | +0.0838 [+0.0669, +0.1024] | 1.2e-21 | 159/30/11 | -0.0001 [-0.0110, +0.0100] |
| Token recall (abstract+body) | +0.0346 [+0.0279, +0.0426] | 4.4e-24 | 145/13/42 | +0.0125 [+0.0071, +0.0187] |
| Word error rate | -0.1480 [-0.1773, -0.1204] | 8.1e-27 | 169/20/11 | -0.0033 [-0.0196, +0.0135] |
| Bag-of-words F1 (order-free) | +0.0573 [+0.0464, +0.0689] | 3.3e-26 | 168/19/13 | -0.0046 [-0.0105, +0.0005] |
| Kendall's tau (paragraph order) | +0.0045 [+0.0003, +0.0093] | 0.00031 | 42/9/148 | +0.0028 [+0.0009, +0.0052] |
| Adjacent paragraph pairs in order | +0.0022 [+0.0004, +0.0042] | 0.00073 | 42/8/149 | +0.0034 [+0.0016, +0.0054] |
| Interleaving events / paper | -1.1100 [-1.6700, -0.5700] | 1.8e-05 | 40/11/149 | -2.0100 [-3.7100, -0.6950] |
| Broken words / 1000 tokens | -3.3572 [-5.1121, -1.8773] | 4.6e-05 | 75/48/77 | -0.3329 [-0.7703, -0.0563] |
|   spurious splits / 1k | -3.3461 [-5.0503, -1.8560] | 8.9e-09 | 60/18/122 | -0.2627 [-0.6761, -0.0319] |
|   spurious joins / 1k | -0.0524 [-0.1757, +0.0142] | 0.47 | 30/38/132 | -0.0363 [-0.1217, +0.0722] |
|   leftover line-break hyphens / 1k | +0.0413 [-0.0089, +0.1030] | 0.0084 | 11/21/168 | -0.0338 [-0.0956, -0.0000] |
| Lost compound hyphens / 1000 tokens | -0.0179 [-0.0635, +0.0216] | 0.84 | 77/82/41 | +0.0006 [-0.0612, +0.0469] |
| Boilerplate leaks / paper | -0.7950 [-1.0450, -0.5750] | 1.3e-12 | 74/4/122 | +0.1950 [-0.0150, +0.4850] |
|   running headers/footers | -0.1900 [-0.3000, -0.1000] | 2.7e-05 | 21/0/179 | +0.0750 [-0.0300, +0.2600] |
|   page numbers | -0.0900 [-0.1650, -0.0250] | 0.0046 | 16/2/182 | +0.1300 [+0.0000, +0.3550] |
|   publisher/licence lines | -0.5150 [-0.6600, -0.3800] | 4.5e-11 | 61/3/136 | -0.0100 [-0.0300, +0.0100] |
| Papers with any leak (share) | -0.2250 [-0.2900, -0.1650] | 3e-10 | 48/3/149 | +0.0050 [-0.0150, +0.0300] |

## Figures, tables, equations, references (pooled over papers [95% CI])

| | Swift (current) | Swift (baseline 38d2bdc^) | Rust (PDFium) |
|---|---|---|---|
| Figure images, count-based recall Σmin(out,xml)/Σxml | 96.0 [91.3, 99.0] | 96.2 [91.6, 99.2] | 96.0 [91.4, 99.1] |
| Figure images, count-based precision Σmin(out,xml)/Σout | 72.7 [67.4, 78.2] | 62.5 [57.3, 67.9] | 72.5 [67.1, 78.2] |
| Figure recall, identified (image whose caption/label matches an XML figure) | 66.4 [59.0, 73.4] | 65.7 [58.6, 72.5] | 72.2 [65.6, 79.0] |
| Figure precision (output figures matching an XML figure) | 50.3 [44.1, 56.7] | 42.7 [37.5, 48.1] | 54.5 [48.2, 61.1] |
| Figure captions kept (≥80 % of first 50 words) | 61.0 [53.9, 68.3] | 55.0 [47.8, 62.2] | 60.4 [52.4, 68.1] |
| Table images, count-based recall | 68.0 [61.0, 75.0] | 62.0 [55.1, 68.8] | 68.6 [61.1, 75.6] |
| Table recall, identified (kept as image, label/caption matches) | 65.9 [58.5, 72.9] | 56.0 [49.5, 62.6] | 66.7 [59.2, 73.8] |
| Table precision | 94.1 [90.9, 97.0] | 87.7 [83.7, 91.4] | 94.4 [91.2, 97.4] |
| Table captions kept | 72.6 [66.0, 78.7] | 64.0 [57.3, 70.4] | 73.8 [67.2, 79.7] |
| Display equations kept as images (min(out,xml)/xml) | 43.7 [20.2, 49.2] | 52.0 [23.7, 58.3] | 40.4 [25.3, 51.9] |
| Exact figure count (% papers) | 47.0 | 30.5 | 45.5 |
| Figures: XML total / output total | 849 / 1121 | 849 / 1308 | 849 / 1124 |
| Tables: XML total / output total | 484 / 339 | 484 / 309 | 484 / 342 |
| Display equations: XML total / output images | 277 / 130 | 277 / 218 | 277 / 135 |
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
| swift->xml (n=200 papers) | 73.0 | 6.5 | 8.5 | 2.7 | 9.2 | 89.5 [87.7, 91.0] | 1.3 |
| base->xml (n=200 papers) | 66.3 | 5.1 | 9.9 | 3.8 | 14.9 | 82.9 [80.6, 84.9] | 2.3 |
| rust->xml (n=200 papers) | 76.0 | 5.8 | 7.6 | 2.0 | 8.6 | 90.5 [88.8, 92.0] | 0.9 |
| base->swift (n=200 papers) | 90.0 | 0.7 | 3.3 | 0.9 | 5.0 | 94.1 [92.4, 95.6] | 0.8 |

Author manuscripts

| Scenario | exact | whitespace | fold | start/end | lost | anchored correctly | anchored to wrong text |
|---|---|---|---|---|---|---|---|
| swift->xml (n=30 papers) | 89.3 | 4.3 | 2.0 | 1.2 | 3.2 | 96.5 [94.7, 98.2] | 0.3 |
| base->xml (n=30 papers) | 89.7 | 4.3 | 2.2 | 0.8 | 3.0 | 96.0 [94.3, 97.5] | 1.0 |
| rust->xml (n=30 papers) | 91.7 | 4.3 | 0.7 | 0.7 | 2.7 | 96.8 [95.0, 98.5] | 0.5 |
| base->swift (n=30 papers) | 99.2 | 0.2 | 0.7 | 0.0 | 0.0 | 100.0 [100.0, 100.0] | 0.0 |

## Speed

| System | setting | s/page median (IQR) | CPU s/page | s/paper | peak RSS MB |
|---|---|---|---|---|---|
| Swift (current) | isolated (one conversion at a time) | 0.124 (0.105–0.142) | 0.085 (0.077–0.091) | 1.17 | 75 |
| Rust (PDFium) | isolated (one conversion at a time) | 0.047 (0.031–0.065) | 0.046 (0.030–0.064) | 0.42 | 103 |
| Swift (baseline 38d2bdc^) | batch (3 conversions in parallel) | 0.190 (0.146–0.254) | 0.085 (0.074–0.097) | 2.02 | 75 |

Batch run (3 conversions in parallel, all 200 papers): Swift (current) 0.129 s/page wall, 0.092 s/page CPU; Swift (baseline 38d2bdc^) 0.190 s/page wall, 0.085 s/page CPU; Rust (PDFium) 0.067 s/page wall, 0.059 s/page CPU.

## Median token F1 / broken words per 1k by year stratum

| Group | n | F1 Swift current | F1 baseline | F1 Rust | broken/1k current | broken/1k baseline | broken/1k Rust |
|---|---|---|---|---|---|---|---|
| 2010-2014 | 40 | 0.943 | 0.827 | 0.959 | 0.00 | 0.00 | 0.00 |
| 2015-2018 | 40 | 0.967 | 0.898 | 0.972 | 0.25 | 0.28 | 0.00 |
| 2019-2021 | 40 | 0.957 | 0.881 | 0.962 | 0.00 | 0.29 | 0.00 |
| 2022-2026 | 40 | 0.960 | 0.873 | 0.971 | 0.00 | 0.55 | 0.00 |
| <=2009 | 40 | 0.947 | 0.932 | 0.899 | 0.23 | 0.28 | 0.31 |

## Median token F1 / broken words per 1k by publisher (≥ 5 papers)

| Group | n | F1 Swift current | F1 baseline | F1 Rust | broken/1k current | broken/1k baseline | broken/1k Rust |
|---|---|---|---|---|---|---|---|
| BMC | 54 | 0.956 | 0.844 | 0.947 | 0.19 | 0.17 | 0.16 |
| Multidisciplinary Digital Publishing Institute (MDPI) | 26 | 0.983 | 0.858 | 0.985 | 0.00 | 16.34 | 0.00 |
| Wiley | 22 | 0.954 | 0.916 | 0.979 | 0.33 | 0.37 | 0.00 |
| (none) | 15 | 0.958 | 0.923 | 0.970 | 0.17 | 0.17 | 0.00 |
| Frontiers Media SA | 12 | 0.844 | 0.834 | 0.921 | 0.00 | 0.00 | 0.00 |
| PLOS | 6 | 0.909 | 0.893 | 0.912 | 0.05 | 0.05 | 0.00 |
| Nature Publishing Group | 5 | 0.951 | 0.893 | 0.959 | 0.61 | 0.69 | 0.18 |

## Swift vs Rust agreement (token F1 between the two outputs)

- swift-rust: 0.992 [0.989, 0.995] (IQR 0.963–0.998)
- swift-base: 0.960 [0.943, 0.967] (IQR 0.865–0.985)
- base-rust: 0.923 [0.902, 0.935] (IQR 0.840–0.971)
- Pearson r of per-paper F1 vs XML (Swift, Rust): 0.83
