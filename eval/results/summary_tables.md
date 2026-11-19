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
| Token F1 (LCS, vs XML) | 0.955 (0.918–0.979); mean 0.927 [0.913, 0.940] | 0.890 (0.798–0.948); mean 0.860 [0.841, 0.877] | 0.960 (0.913–0.982); mean 0.932 [0.919, 0.943] | 1.000 (0.999–1.000); mean 0.999 [0.998, 0.999] |
| Token precision | 0.940 (0.878–0.971); mean 0.905 [0.890, 0.918] | 0.861 (0.728–0.945); mean 0.821 [0.801, 0.841] | 0.939 (0.878–0.971); mean 0.905 [0.889, 0.919] | 0.999 (0.998–1.000); mean 0.997 [0.997, 0.998] |
| Token recall (abstract+body) | 0.987 (0.967–0.996); mean 0.959 [0.946, 0.970] | 0.966 (0.906–0.986); mean 0.924 [0.907, 0.940] | 0.993 (0.976–0.997); mean 0.971 [0.960, 0.981] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Word error rate | 0.087 (0.043–0.170); mean 0.165 [0.125, 0.226] | 0.221 (0.106–0.415); mean 0.312 [0.264, 0.374] | 0.079 (0.042–0.173); mean 0.161 [0.122, 0.221] | 0.001 (0.000–0.002); mean 0.003 [0.002, 0.004] |
| Bag-of-words F1 (order-free) | 0.968 (0.937–0.984); mean 0.944 [0.932, 0.955] | 0.925 (0.845–0.959); mean 0.887 [0.870, 0.903] | 0.967 (0.929–0.985); mean 0.940 [0.927, 0.951] | 1.000 (0.999–1.000); mean 0.999 [0.998, 0.999] |
| Kendall's tau (paragraph order) | 1.000 (0.997–1.000); mean 0.986 [0.979, 0.992] | 1.000 (0.995–1.000); mean 0.982 [0.973, 0.989] | 1.000 (1.000–1.000); mean 0.989 [0.982, 0.995] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Adjacent paragraph pairs in order | 1.000 (0.983–1.000); mean 0.989 [0.986, 0.992] | 1.000 (0.977–1.000); mean 0.986 [0.983, 0.989] | 1.000 (1.000–1.000); mean 0.992 [0.990, 0.994] | 1.000 (1.000–1.000); mean 1.000 [1.000, 1.000] |
| Interleaving events / paper | 0.000 (0.000–0.000); mean 2.265 [0.965, 3.855] | 0.000 (0.000–1.250); mean 3.375 [2.065, 4.965] | 0.000 (0.000–0.000); mean 0.255 [0.140, 0.405] | 0.000 (0.000–0.000); mean 0.025 [0.000, 0.070] |
| Broken words / 1000 tokens | 0.114 (0.000–0.580); mean 0.804 [0.459, 1.289] | 0.269 (0.000–1.086); mean 4.150 [2.647, 5.927] | 0.000 (0.000–0.299); mean 0.472 [0.265, 0.725] | 0.000 (0.000–0.000); mean 0.135 [0.067, 0.231] |
|   spurious splits / 1k | 0.000 (0.000–0.000); mean 0.519 [0.177, 0.985] | 0.000 (0.000–0.585); mean 3.865 [2.379, 5.648] | 0.000 (0.000–0.000); mean 0.257 [0.071, 0.499] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.001] |
|   spurious joins / 1k | 0.000 (0.000–0.186); mean 0.180 [0.125, 0.245] | 0.000 (0.000–0.165); mean 0.233 [0.130, 0.379] | 0.000 (0.000–0.000); mean 0.144 [0.075, 0.245] | 0.000 (0.000–0.000); mean 0.135 [0.066, 0.233] |
|   leftover line-break hyphens / 1k | 0.000 (0.000–0.000); mean 0.105 [0.047, 0.181] | 0.000 (0.000–0.000); mean 0.051 [0.026, 0.086] | 0.000 (0.000–0.000); mean 0.071 [0.033, 0.117] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
| Lost compound hyphens / 1000 tokens | 0.578 (0.193–1.152); mean 0.792 [0.687, 0.903] | 0.584 (0.253–1.208); mean 0.809 [0.704, 0.919] | 0.585 (0.200–1.173); mean 0.792 [0.697, 0.895] | 0.000 (0.000–0.000); mean 0.002 [0.000, 0.005] |
| Boilerplate leaks / paper | 0.000 (0.000–1.000); mean 0.845 [0.550, 1.230] | 1.000 (0.000–2.000); mean 1.625 [1.235, 2.080] | 0.000 (0.000–1.000); mean 1.045 [0.615, 1.625] | 0.000 (0.000–0.000); mean 0.115 [0.000, 0.345] |
|   running headers/footers | 0.000 (0.000–0.000); mean 0.145 [0.070, 0.245] | 0.000 (0.000–0.000); mean 0.330 [0.200, 0.485] | 0.000 (0.000–0.000); mean 0.220 [0.080, 0.430] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
|   page numbers | 0.000 (0.000–0.000); mean 0.335 [0.080, 0.705] | 0.000 (0.000–0.000); mean 0.425 [0.160, 0.795] | 0.000 (0.000–0.000); mean 0.475 [0.120, 1.055] | 0.000 (0.000–0.000); mean 0.115 [0.000, 0.345] |
|   publisher/licence lines | 0.000 (0.000–1.000); mean 0.365 [0.270, 0.475] | 1.000 (0.000–1.000); mean 0.870 [0.715, 1.030] | 0.000 (0.000–1.000); mean 0.350 [0.260, 0.455] | 0.000 (0.000–0.000); mean 0.000 [0.000, 0.000] |
| Papers with any boilerplate leak (%) | 41.5 [34.5, 48.5] | 63.5 [56.5, 70.0] | 42.0 [35.5, 49.0] | 0.5 [0.0, 1.5] |
| Papers with any interleaving (%) | 20.5 [15.0, 26.0] | 34.0 [27.5, 40.5] | 12.5 [8.0, 17.5] | 1.0 [0.0, 2.5] |
| Papers with token F1 < 0.9 (%) | 20.5 [15.0, 26.0] | 53.5 [46.5, 60.5] | 21.5 [16.0, 27.5] | 0.0 [0.0, 0.0] |

## Current vs baseline (paired, current − baseline; and Rust − Swift)

| Metric | Δ mean [95% CI] (Swift current − baseline) | Wilcoxon p | better/worse/same | Δ mean [95% CI] (Rust − Swift) |
|---|---|---|---|---|
| Token F1 (LCS, vs XML) | +0.0672 [+0.0559, +0.0796] | 4.1e-28 | 171/18/11 | +0.0049 [-0.0028, +0.0131] |
| Token precision | +0.0833 [+0.0665, +0.1020] | 1.8e-21 | 159/30/11 | +0.0001 [-0.0109, +0.0102] |
| Token recall (abstract+body) | +0.0343 [+0.0275, +0.0422] | 9.5e-24 | 143/13/44 | +0.0126 [+0.0072, +0.0188] |
| Word error rate | -0.1474 [-0.1764, -0.1199] | 1e-26 | 169/20/11 | -0.0037 [-0.0200, +0.0132] |
| Bag-of-words F1 (order-free) | +0.0570 [+0.0461, +0.0686] | 3.9e-26 | 168/19/13 | -0.0044 [-0.0103, +0.0007] |
| Kendall's tau (paragraph order) | +0.0044 [+0.0003, +0.0093] | 0.00035 | 42/9/148 | +0.0028 [+0.0009, +0.0052] |
| Adjacent paragraph pairs in order | +0.0022 [+0.0004, +0.0042] | 0.00073 | 42/8/149 | +0.0034 [+0.0016, +0.0054] |
| Interleaving events / paper | -1.1100 [-1.6700, -0.5700] | 1.8e-05 | 40/11/149 | -2.0100 [-3.7100, -0.6950] |
| Broken words / 1000 tokens | -3.3463 [-5.1014, -1.8673] | 3.1e-05 | 76/47/77 | -0.3317 [-0.7685, -0.0557] |
|   spurious splits / 1k | -3.3465 [-5.0503, -1.8567] | 8.4e-09 | 60/18/122 | -0.2623 [-0.6751, -0.0318] |
|   spurious joins / 1k | -0.0531 [-0.1766, +0.0135] | 0.63 | 31/37/132 | -0.0356 [-0.1209, +0.0735] |
|   leftover line-break hyphens / 1k | +0.0534 [+0.0116, +0.1133] | 0.0016 | 10/22/168 | -0.0338 [-0.0956, -0.0000] |
| Lost compound hyphens / 1000 tokens | -0.0167 [-0.0626, +0.0230] | 0.91 | 76/83/41 | -0.0006 [-0.0623, +0.0454] |
| Boilerplate leaks / paper | -0.7800 [-1.0300, -0.5600] | 1.9e-12 | 73/4/123 | +0.2000 [-0.0100, +0.4950] |
|   running headers/footers | -0.1850 [-0.2950, -0.0950] | 4.4e-05 | 20/0/180 | +0.0750 [-0.0300, +0.2600] |
|   page numbers | -0.0900 [-0.1600, -0.0300] | 0.002 | 16/1/183 | +0.1400 [+0.0100, +0.3650] |
|   publisher/licence lines | -0.5050 [-0.6500, -0.3700] | 6.5e-11 | 60/3/137 | -0.0150 [-0.0350, +0.0000] |
| Papers with any leak (share) | -0.2200 [-0.2850, -0.1600] | 4.9e-10 | 47/3/150 | +0.0050 [-0.0100, +0.0250] |

## Figures, tables, equations, references (pooled over papers [95% CI])

| | Swift (current) | Swift (baseline 38d2bdc^) | Rust (PDFium) |
|---|---|---|---|
| Figure images, count-based recall Σmin(out,xml)/Σxml | 96.0 [91.3, 99.0] | 96.2 [91.6, 99.2] | 96.0 [91.4, 99.1] |
| Figure images, count-based precision Σmin(out,xml)/Σout | 72.7 [67.4, 78.2] | 62.5 [57.3, 67.9] | 72.5 [67.1, 78.2] |
| Figure recall, identified (image whose caption/label matches an XML figure) | 66.0 [58.6, 73.0] | 65.7 [58.6, 72.5] | 72.2 [65.6, 79.0] |
| Figure precision (output figures matching an XML figure) | 50.0 [43.8, 56.1] | 42.7 [37.5, 48.1] | 54.5 [48.2, 61.1] |
| Figure captions kept (≥80 % of first 50 words) | 61.0 [53.9, 68.3] | 55.0 [47.8, 62.2] | 60.4 [52.4, 68.1] |
| Table images, count-based recall | 68.0 [61.0, 75.0] | 62.0 [55.1, 68.8] | 68.6 [61.1, 75.6] |
| Table recall, identified (kept as image, label/caption matches) | 65.9 [58.5, 72.9] | 56.0 [49.5, 62.6] | 66.7 [59.2, 73.8] |
| Table precision | 94.1 [90.9, 97.0] | 87.7 [83.7, 91.4] | 94.4 [91.2, 97.4] |
| Table captions kept | 72.6 [66.0, 78.7] | 64.0 [57.3, 70.4] | 73.8 [67.2, 79.7] |
| Display equations kept as images (min(out,xml)/xml) | 44.4 [22.4, 49.6] | 52.0 [23.7, 58.3] | 39.0 [24.2, 50.6] |
| Exact figure count (% papers) | 47.0 | 30.5 | 45.5 |
| Figures: XML total / output total | 849 / 1121 | 849 / 1308 | 849 / 1124 |
| Tables: XML total / output total | 484 / 339 | 484 / 309 | 484 / 342 |
| Display equations: XML total / output images | 277 / 132 | 277 / 218 | 277 / 131 |
| Reference items out/XML (median ratio) | 1.00 | 0.85 | 1.00 |

## Figures cut from the PDF for an XML version without images

| Subset | Papers | Figures recovered (pooled [95% CI]) | Papers with all figures | Papers with none |
|---|---|---|---|---|
| author manuscripts | 30/30 | 96.9 [90.8, 100.0] | 93.3 % | 3.3 % |
| main corpus | 200/200 | 49.1 [40.4, 57.7] | 47.0 % | 55.0 % |

## Highlight survival (20 simulated highlights per paper; % of highlights [95% CI])


Main corpus (paired set)

| Scenario | exact | whitespace | fold | start/end | lost | anchored correctly | anchored to wrong text |
|---|---|---|---|---|---|---|---|
| swift->xml (n=200 papers) | 72.8 | 6.4 | 8.3 | 2.7 | 9.8 | 88.9 [87.2, 90.5] | 1.3 |
| base->xml (n=200 papers) | 66.3 | 5.1 | 9.9 | 3.8 | 14.9 | 82.9 [80.6, 84.9] | 2.3 |
| rust->xml (n=200 papers) | 76.0 | 5.9 | 7.5 | 2.2 | 8.4 | 90.6 [88.9, 92.0] | 1.0 |
| base->swift (n=200 papers) | 90.8 | 0.7 | 3.3 | 0.6 | 4.5 | 94.7 [93.0, 96.1] | 0.8 |

Author manuscripts

| Scenario | exact | whitespace | fold | start/end | lost | anchored correctly | anchored to wrong text |
|---|---|---|---|---|---|---|---|
| swift->xml (n=30 papers) | 89.3 | 4.3 | 2.0 | 1.2 | 3.2 | 96.5 [94.7, 98.2] | 0.3 |
| base->xml (n=30 papers) | 89.7 | 4.3 | 2.2 | 0.8 | 3.0 | 96.0 [94.3, 97.5] | 1.0 |
| rust->xml (n=30 papers) | 91.5 | 4.3 | 0.7 | 0.7 | 2.8 | 96.7 [94.8, 98.3] | 0.5 |
| base->swift (n=30 papers) | 99.2 | 0.2 | 0.7 | 0.0 | 0.0 | 100.0 [100.0, 100.0] | 0.0 |

## Speed

| System | setting | s/page median (IQR) | CPU s/page | s/paper | peak RSS MB |
|---|---|---|---|---|---|
| Swift (current) | isolated (one conversion at a time) | 0.124 (0.105–0.142) | 0.085 (0.077–0.091) | 1.17 | 75 |
| Rust (PDFium) | isolated (one conversion at a time) | 0.047 (0.031–0.065) | 0.046 (0.030–0.064) | 0.42 | 103 |
| Swift (baseline 38d2bdc^) | batch (3 conversions in parallel) | 0.190 (0.146–0.254) | 0.085 (0.074–0.097) | 2.02 | 75 |

Batch run (3 conversions in parallel, all 200 papers): Swift (current) 0.126 s/page wall, 0.090 s/page CPU; Swift (baseline 38d2bdc^) 0.190 s/page wall, 0.085 s/page CPU; Rust (PDFium) 0.044 s/page wall, 0.043 s/page CPU.

## Median token F1 / broken words per 1k by year stratum

| Group | n | F1 Swift current | F1 baseline | F1 Rust | broken/1k current | broken/1k baseline | broken/1k Rust |
|---|---|---|---|---|---|---|---|
| 2010-2014 | 40 | 0.943 | 0.827 | 0.959 | 0.00 | 0.00 | 0.00 |
| 2015-2018 | 40 | 0.967 | 0.898 | 0.972 | 0.26 | 0.28 | 0.00 |
| 2019-2021 | 40 | 0.957 | 0.881 | 0.962 | 0.00 | 0.29 | 0.00 |
| 2022-2026 | 40 | 0.960 | 0.873 | 0.971 | 0.00 | 0.55 | 0.00 |
| <=2009 | 40 | 0.947 | 0.932 | 0.899 | 0.23 | 0.28 | 0.31 |

## Median token F1 / broken words per 1k by publisher (≥ 5 papers)

| Group | n | F1 Swift current | F1 baseline | F1 Rust | broken/1k current | broken/1k baseline | broken/1k Rust |
|---|---|---|---|---|---|---|---|
| BMC | 54 | 0.956 | 0.844 | 0.947 | 0.19 | 0.17 | 0.16 |
| Multidisciplinary Digital Publishing Institute (MDPI) | 26 | 0.983 | 0.858 | 0.985 | 0.00 | 16.34 | 0.00 |
| Wiley | 22 | 0.954 | 0.916 | 0.976 | 0.33 | 0.37 | 0.00 |
| (none) | 15 | 0.953 | 0.923 | 0.967 | 0.17 | 0.17 | 0.00 |
| Frontiers Media SA | 12 | 0.844 | 0.834 | 0.921 | 0.00 | 0.00 | 0.00 |
| PLOS | 6 | 0.908 | 0.893 | 0.911 | 0.05 | 0.05 | 0.00 |
| Nature Publishing Group | 5 | 0.951 | 0.893 | 0.959 | 0.61 | 0.69 | 0.18 |

## Swift vs Rust agreement (token F1 between the two outputs)

- swift-rust: 0.993 [0.990, 0.995] (IQR 0.963–0.998)
- swift-base: 0.960 [0.945, 0.967] (IQR 0.865–0.987)
- base-rust: 0.923 [0.903, 0.935] (IQR 0.840–0.971)
- Pearson r of per-paper F1 vs XML (Swift, Rust): 0.83
