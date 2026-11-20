// Word documents with citations from Zotero, EndNote and Mendeley, as their add-ins write them
// (the formats: see apple/citations/word/fields.js): the OOXML body.getOoxml() answers and the
// field codes Word's Field objects report, in the same order.

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const b64 = (s, enc) => Buffer.from(s, enc || 'utf8').toString('base64');

// Zotero 6/7: the JSON right after CSL_CITATION, split over three runs where the formatting changes
const ZOTERO_JSON = JSON.stringify({
  citationID: 'Ab3dE9xK',
  properties: { formattedCitation: '(see Smith, 2020, p. 12; Wu, 2019)', plainCitation: '(see Smith, 2020, p. 12; Wu, 2019)', noteIndex: 0 },
  citationItems: [
    { id: 1234, uris: ['http://zotero.org/users/123456/items/ABCD2345'],
      itemData: { id: 1234, type: 'article-journal', 'container-title': 'Cell', DOI: '10.1000/ALPHA', issue: '3', page: '1-10', title: 'Alpha', volume: '5',
        author: [{ family: 'Smith', given: 'Jane' }], issued: { 'date-parts': [['2020', 3, 1]] } },
      locator: '12', label: 'page', prefix: 'see' },
    { id: 99, uris: ['http://zotero.org/users/123456/items/XYZ98765'],
      itemData: { id: 99, type: 'article-journal', title: 'Gamma <&>', author: [{ family: 'Wu', given: 'Per' }], issued: { 'date-parts': [['2019']] } } }
  ],
  schema: 'https://github.com/citation-style-language/schema/raw/master/csl-citation.json'
});
const ZOTERO_CODE = ' ADDIN ZOTERO_ITEM CSL_CITATION ' + ZOTERO_JSON + ' ';

// EndNote, inline: the Cite XML in the field code (ExcludeAuth: without the author; Pages: the cited page)
const EN_INLINE_XML = '<EndNote><Cite ExcludeAuth="1"><Author>Smith</Author><Year>2020</Year><RecNum>12</RecNum><Pages>4</Pages>' +
  '<DisplayText>(2020, p. 4)</DisplayText><record><rec-number>12</rec-number><foreign-keys><key app="EN" db-id="z2asfrdsndpezaeta27x5ff5d022r5x9x9x0" timestamp="1483621777">12</key></foreign-keys>' +
  '<ref-type name="Journal Article">17</ref-type><contributors><authors><author><style face="normal" font="default" size="100%">Smith, Jane</style></author></authors></contributors>' +
  '<titles><title><style face="normal" font="default" size="100%">Beta</style></title><secondary-title>Neuron</secondary-title></titles>' +
  '<pages>5-9</pages><volume>2</volume><dates><year>2020</year><pub-dates><date>2020/001/001</date></pub-dates></dates></record></Cite></EndNote>';
const EN_INLINE_CODE = ' ADDIN EN.CITE ' + EN_INLINE_XML;

// EndNote, long: the XML base64 in <w:fldData>, an EN.CITE.DATA field nested in the EN.CITE one
const EN_DATA_XML = '<EndNote><Cite><Author>Rao</Author><Year>2023</Year><RecNum>7</RecNum><Prefix>e.g.`</Prefix><Suffix>`, for the method</Suffix>' +
  '<DisplayText>(e.g. Rao et al., 2023, for the method)</DisplayText><record><rec-number>7</rec-number><ref-type name="Journal Article">17</ref-type>' +
  '<contributors><authors><author>Rao, Priya</author><author>Lindqvist, Åsa</author><author>Ng, Kim</author></authors></contributors>' +
  '<titles><title>Delta rays &amp; cells</title><secondary-title>Nature Methods</secondary-title></titles><periodical><full-title>Nature Methods</full-title></periodical>' +
  '<pages>100-110</pages><volume>20</volume><number>1</number><dates><year>2023</year></dates><accession-num>36543210</accession-num>' +
  '<electronic-resource-num>10.5555/delta</electronic-resource-num></record></Cite></EndNote>';

// Mendeley Desktop (until 2022)
const MENDELEY_JSON = JSON.stringify({
  citationItems: [{ id: 'ITEM-1', itemData: { author: [{ 'dropping-particle': '', family: 'Okafor', given: 'Ngozi', 'non-dropping-particle': '', 'parse-names': false, suffix: '' }],
    id: 'ITEM-1', issued: { 'date-parts': [['2018']] }, title: 'Epsilon waves in sleep', type: 'article-journal' }, uris: ['http://www.mendeley.com/documents/?uuid=1f2e3d4c'] }],
  mendeley: { formattedCitation: '(Okafor, 2018)', plainTextFormattedCitation: '(Okafor, 2018)', previouslyFormattedCitation: '(Okafor, 2018)' },
  properties: { noteIndex: 0 }, schema: 'https://github.com/citation-style-language/schema/raw/master/csl-citation.json'
});
const MENDELEY_CODE = ' ADDIN CSL_CITATION ' + MENDELEY_JSON;

// Mendeley Cite (the add-in since 2022): a content control whose tag is the base64 JSON
const MENDELEY_CITE_TAG = 'MENDELEY_CITATION_v3_' + b64(JSON.stringify({
  citationID: 'MENDELEY_CITATION_1de18c13-0c8d-4a3b-9e70-96407afdc71e', properties: { noteIndex: 0 }, isEdited: false,
  manualOverride: { isManuallyOverridden: false, citeprocText: '(Smith, 2020)', manualOverrideText: '' },
  citationItems: [{ id: 'e219b34e-d397-37b4-818e-80ca578eae74', itemData: { type: 'article-journal', id: 'e219b34e-d397-37b4-818e-80ca578eae74', title: 'Beta',
    author: [{ family: 'Smith', given: 'Jane', 'parse-names': false, 'dropping-particle': '', 'non-dropping-particle': '' }], issued: { 'date-parts': [[2020, 9, 1]] } },
    isTemporary: false, locator: '7', label: 'figure', 'suppress-author': false, composite: false, 'author-only': false }]
}));

const ZOTERO_BIBL_CODE = ' ADDIN ZOTERO_BIBL {"uncited":[],"omitted":[],"custom":[]} CSL_BIBLIOGRAPHY ';

const r = (inner, props) => '<w:r>' + (props ? '<w:rPr>' + props + '</w:rPr>' : '') + inner + '</w:r>';
const t = (s) => r('<w:t xml:space="preserve">' + esc(s) + '</w:t>');
const begin = (data) => r(data ? '<w:fldChar w:fldCharType="begin"><w:fldData xml:space="preserve">' + data.replace(/(.{76})/g, '$1\n') + '</w:fldData></w:fldChar>' : '<w:fldChar w:fldCharType="begin"/>');
const instr = (s, props) => r('<w:instrText xml:space="preserve">' + esc(s) + '</w:instrText>', props);
const sep = r('<w:fldChar w:fldCharType="separate"/>'), end = r('<w:fldChar w:fldCharType="end"/>');
const P = (...kids) => '<w:p><w:pPr><w:pStyle w:val="Normal"/></w:pPr>' + kids.join('') + '</w:p>';

function documentXML() {
  const z = ZOTERO_CODE, cut1 = 120, cut2 = 300;
  const body =
    P(t('Known '), begin(), instr(z.slice(0, cut1)), instr(z.slice(cut1, cut2), '<w:rFonts w:ascii="Cambria Math"/>'), instr(z.slice(cut2)), sep, t('(see Smith, 2020, p. 12; Wu, 2019)'), end,
      t(' and a '), '<w:fldSimple w:instr=" HYPERLINK &quot;https://example.org&quot; ">' + t('link') + '</w:fldSimple>', t('.')) +
    P(t('Beta '), begin(), instr(EN_INLINE_CODE), sep, t('(2020, p. 4)'), end, t('; delta '),
      begin(b64(EN_DATA_XML)), instr(' ADDIN EN.CITE '), begin(b64(EN_DATA_XML)), instr(' ADDIN EN.CITE.DATA '), end, sep, t('(e.g. Rao et al., 2023, for the method)'), end) +
    P(t('Sleep '), begin(), instr(MENDELEY_CODE), sep, t('(Okafor, 2018)'), end, t(' and '),
      '<w:sdt><w:sdtPr><w:tag w:val="' + MENDELEY_CITE_TAG + '"/><w:id w:val="-1410928299"/></w:sdtPr><w:sdtContent>' + t('(Smith, 2020)') + '</w:sdtContent></w:sdt>') +
    P(r('<w:t>References</w:t>')) +
    '<w:p><w:pPr><w:pStyle w:val="Bibliography"/></w:pPr>' + begin() + instr(ZOTERO_BIBL_CODE) + sep + t('Smith, J. (2020). Alpha. Cell.') + '</w:p>' +
    '<w:p><w:pPr><w:pStyle w:val="Bibliography"/></w:pPr>' + t('Wu, P. (2019). Gamma.') + end + '</w:p>' +
    '<w:p><w:pPr><w:pStyle w:val="EndNoteBibliography"/></w:pPr>' + begin() + instr(' ADDIN EN.REFLIST ') + sep + t('Rao P (2023) Delta rays.') + end + '</w:p>';
  return '<pkg:package xmlns:pkg="http://schemas.microsoft.com/office/2006/xmlPackage"><pkg:part pkg:name="/word/document.xml"><pkg:xmlData>' +
    '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>' + body +
    '<w:sectPr/></w:body></w:document></pkg:xmlData></pkg:part></pkg:package>';
}

/** The fields and content controls in document order, as Word's API has them. */
const NODES = [
  { field: ZOTERO_CODE, name: 'zotero' },
  { field: ' HYPERLINK "https://example.org" ' },
  { field: EN_INLINE_CODE },
  { field: ' ADDIN EN.CITE ', name: 'en-outer' },
  { field: ' ADDIN EN.CITE.DATA ', parent: 'en-outer' },
  { field: MENDELEY_CODE },
  [MENDELEY_CITE_TAG, '(Smith, 2020)'],
  { field: ZOTERO_BIBL_CODE, name: 'zbib' },
  { field: ' ADDIN EN.REFLIST ' }
];

/** Long EndNote citations, one after another (each the XML base64 in <w:fldData>, an EN.CITE.DATA
 *  field nested in the EN.CITE one), then EndNote's bibliography: {xml, nodes} with Word listing
 *  the nested fields as fields of their own. */
function endnoteLong(titles) {
  const cite = (title, i) => '<EndNote><Cite><Author>Author' + i + '</Author><Year>2021</Year><RecNum>' + (40 + i) + '</RecNum>' +
    '<DisplayText>(Author' + i + ', 2021)</DisplayText><record><rec-number>' + (40 + i) + '</rec-number><ref-type name="Journal Article">17</ref-type>' +
    '<contributors><authors><author>Author' + i + ', Ann</author></authors></contributors><titles><title>' + esc(title) + '</title></titles>' +
    '<dates><year>2021</year></dates><electronic-resource-num>10.5555/long' + i + '</electronic-resource-num></record></Cite></EndNote>';
  const paras = titles.map((title, i) => P(t('Long ' + i + ' '), begin(b64(cite(title, i))), instr(' ADDIN EN.CITE '), begin(b64(cite(title, i))), instr(' ADDIN EN.CITE.DATA '), end,
    sep, t('(Author' + i + ', 2021)'), end)).join('') +
    '<w:p>' + begin() + instr(' ADDIN EN.REFLIST ') + sep + t('Author0 A (2021).') + end + '</w:p>';
  const xml = '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>' + paras + '<w:sectPr/></w:body></w:document>';
  const nodes = [];
  titles.forEach((_, i) => { nodes.push({ field: ' ADDIN EN.CITE ', name: 'en' + i }); nodes.push({ field: ' ADDIN EN.CITE.DATA ', parent: 'en' + i }); });
  nodes.push({ field: ' ADDIN EN.REFLIST ' });
  return { xml, nodes };
}

/** An EndNote citation with nothing in it (unreadable), a readable one, and EndNote's bibliography. */
function endnoteUnreadable() {
  const xml = '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>' +
    P(t('Lost '), begin(), instr(' ADDIN EN.CITE '), sep, t('(Gone, 1999)'), end, t(' and '), begin(), instr(EN_INLINE_CODE), sep, t('(2020, p. 4)'), end) +
    '<w:p>' + begin() + instr(' ADDIN EN.REFLIST ') + sep + t('Gone (1999). Smith (2020).') + end + '</w:p><w:sectPr/></w:body></w:document>';
  return { xml, nodes: [{ field: ' ADDIN EN.CITE ' }, { field: EN_INLINE_CODE }, { field: ' ADDIN EN.REFLIST ' }] };
}

module.exports = { documentXML, NODES, endnoteLong, endnoteUnreadable, ZOTERO_CODE, EN_INLINE_CODE, EN_INLINE_XML, EN_DATA_XML, MENDELEY_CODE, MENDELEY_CITE_TAG, ZOTERO_BIBL_CODE, b64 };
