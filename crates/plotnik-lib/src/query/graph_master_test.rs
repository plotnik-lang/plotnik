//! Golden master test for graph construction and type inference.
//!
//! This test exercises the full spectrum of ADR-specified behaviors:
//! - ADR-0004: Binary format concepts (transitions, effects, strings, types)
//! - ADR-0005: Transition graph (matchers, nav, ref markers, quantifiers)
//! - ADR-0006: Query execution (effect stream, materialization)
//! - ADR-0007: Type metadata (TypeKind, synthetic naming, flattening)
//! - ADR-0008: Tree navigation (Nav kinds, anchor lowering)
//! - ADR-0009: Type system (cardinality, scopes, alternations, QIS, unification)

use indoc::indoc;

use crate::query::Query;

fn golden_master(source: &str) -> String {
    let query = Query::try_from(source)
        .expect("parse should succeed")
        .build_graph();

    let mut out = String::new();

    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════\n",
    );
    out.push_str("                              TRANSITION GRAPH\n");
    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════\n\n",
    );
    out.push_str(&query.graph().dump_live(query.dead_nodes()));

    out.push_str(
        "\n═══════════════════════════════════════════════════════════════════════════════\n",
    );
    out.push_str("                              TYPE INFERENCE\n");
    out.push_str(
        "═══════════════════════════════════════════════════════════════════════════════\n\n",
    );
    out.push_str(&query.type_info().dump());

    out
}

/// Comprehensive test covering all major ADR features.
///
/// Query structure:
/// 1. Basic captures with ::string annotation (ADR-0007, ADR-0009)
/// 2. Field constraints and negated fields (ADR-0005)
/// 3. Anchors - first child, last child, siblings (ADR-0008)
/// 4. Quantifiers - *, +, ? with captures (ADR-0005, ADR-0009)
/// 5. QIS - multiple captures in quantified expr (ADR-0009)
/// 6. Tagged alternations - enum generation (ADR-0007, ADR-0009)
/// 7. Untagged alternations - struct merge (ADR-0009)
/// 8. Captured sequences - nested scopes (ADR-0009)
/// 9. Definition references - Enter/Exit (ADR-0005, ADR-0006)
/// 10. Cardinality propagation and joins (ADR-0009)
/// 11. Single-capture variant flattening (ADR-0007, ADR-0009)
/// 12. Deep nesting with multi-level Up (ADR-0008)
/// 13. Wildcards and string literals (ADR-0005)
#[test]
fn golden_master_comprehensive() {
    let source = indoc! {r#"
        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 1: Basic captures and type annotations
        // ═══════════════════════════════════════════════════════════════════════════

        // Simple node capture → Node type
        SimpleCapture = (identifier) @name

        // String annotation → String type
        StringCapture = (identifier) @name ::string

        // Multiple flat captures → Struct with multiple fields
        MultiCapture = (function
            name: (identifier) @fn_name ::string
            body: (block) @fn_body
        )

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 2: Navigation and anchors (ADR-0008)
        // ═══════════════════════════════════════════════════════════════════════════

        // First child anchor → DownSkipTrivia
        AnchorFirst = (parent . (first_child) @first)

        // Last child anchor → UpSkipTrivia
        AnchorLast = (parent (last_child) @last .)

        // Adjacent siblings → NextSkipTrivia
        AnchorSibling = (parent (a) @left . (b) @right)

        // Deep nesting with multi-level Up
        DeepNest = (a (b (c (d) @deep)))

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 3: Quantifiers (ADR-0005, ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Star quantifier → ArrayStar
        StarQuant = (container (item)* @items)

        // Plus quantifier → ArrayPlus
        PlusQuant = (container (item)+ @items)

        // Optional quantifier → Optional
        OptQuant = (container (item)? @maybe_item)

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 4: QIS - Quantifier-Induced Scope (ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Two captures in quantified node → QIS triggers, creates element struct
        QisNode = (function
            name: (identifier) @name
            body: (block) @body
        )*

        // Two captures in quantified sequence → QIS triggers
        QisSequence = { (key) @key (value) @value }*

        // Single capture → NO QIS, standard cardinality propagation
        NoQis = { (item) @item }*

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 5: Tagged alternations (ADR-0007, ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Tagged at definition root → Definition becomes Enum
        // Single capture per variant → flattened payload
        TaggedRoot = [
            Ok: (success) @val
            Err: (error) @msg ::string
        ]

        // Tagged alternation captured → creates nested Enum
        TaggedCaptured = (wrapper [
            Left: (left_node) @l
            Right: (right_node) @r
        ] @choice)

        // Tagged with multi-capture variant → NOT flattened, creates struct
        TaggedMulti = [
            Simple: (node) @val
            Complex: (pair (key) @k (value) @v)
        ]

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 6: Untagged alternations (ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Symmetric captures → required field
        UntaggedSymmetric = [ (a) @val (b) @val ]

        // Asymmetric captures → both become Optional
        UntaggedAsymmetric = [ (a) @x (b) @y ]

        // Captured untagged → creates struct scope
        UntaggedCaptured = [ (a) @x (b) @y ] @data

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 7: Captured sequences and nested scopes (ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Captured sequence → creates nested struct
        CapturedSeq = (outer { (inner) @x (inner2) @y } @nested)

        // Uncaptured sequence → captures propagate to parent
        UncapturedSeq = (outer { (inner) @x (inner2) @y })

        // Deeply nested scopes
        NestedScopes = { { (a) @a } @inner1 { (b) @b } @inner2 } @outer

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 8: Definition references (ADR-0005, ADR-0006)
        // ═══════════════════════════════════════════════════════════════════════════

        // Base definition
        Identifier = (identifier) @id

        // Reference to definition → Enter/Exit markers
        RefSimple = (Identifier)

        // Captured reference → captures the reference result
        RefCaptured = (Identifier) @captured_id

        // Chained references
        RefChain = (RefSimple)

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 9: Cardinality combinations (ADR-0009)
        // ═══════════════════════════════════════════════════════════════════════════

        // Cardinality in alternation branches
        // Branch 1: @item cardinality 1, Branch 2: @item cardinality +
        // Join produces +
        CardinalityJoin = [ (single) @item (multi (x)+ @item) ]

        // Nested quantifiers
        NestedQuant = ((item)* @inner)+ @outer

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 10: Mixed patterns (comprehensive)
        // ═══════════════════════════════════════════════════════════════════════════

        // Everything combined: field constraints, anchors, quantifiers, alternations
        Complex = (module
            name: (identifier) @mod_name ::string
            . (import)* @imports
            body: (block {
                [
                    Func: (function
                        name: (identifier) @fn_name ::string
                        params: (parameters { (param) @p }* @params)
                        body: (block) @fn_body
                    )
                    Class: (class
                        name: (identifier) @cls_name ::string
                        body: (class_body) @cls_body
                    )
                ]
            }* @items) .
        )

        // ═══════════════════════════════════════════════════════════════════════════
        // SECTION 11: Edge cases
        // ═══════════════════════════════════════════════════════════════════════════

        // Wildcard capture
        WildcardCapture = _ @any

        // String literal (anonymous node)
        StringLiteral = "+" @op

        // No captures → Void type
        NoCaptures = (identifier)

        // Empty alternation branch (unit variant)
        EmptyBranch = [
            Some: (value) @val
            None: (none_marker)
        ]
    "#};

    insta::assert_snapshot!(golden_master(source), @r#"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    SimpleCapture = (000)
    StringCapture = (001)
    MultiCapture = (002)
    AnchorFirst = (010)
    AnchorLast = (013)
    AnchorSibling = (018)
    DeepNest = (026)
    StarQuant = (033)
    PlusQuant = (042)
    OptQuant = (051)
    QisNode = (068)
    QisSequence = (085)
    NoQis = (097)
    TaggedRoot = (100)
    TaggedCaptured = (112)
    TaggedMulti = (126)
    UntaggedSymmetric = (142)
    UntaggedAsymmetric = (150)
    UntaggedCaptured = (158)
    CapturedSeq = (166)
    UncapturedSeq = (175)
    NestedScopes = (188)
    Identifier = (199)
    RefSimple = (200)
    RefCaptured = (202)
    RefChain = (204)
    CardinalityJoin = (206)
    NestedQuant = (222)
    Complex = (242)
    WildcardCapture = (306)
    StringLiteral = (307)
    NoCaptures = (308)
    EmptyBranch = (309)

    (000) —(identifier)—[CaptureNode]→ (✓)
    (001) —(identifier)—[CaptureNode, ToString]→ (✓)
    (002) —(function)—[StartObject]→ (003)
    (003) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (004)
    (004) —𝜀—[Field(fn_name)]→ (005)
    (005) —{→}—(block)@body—[CaptureNode]→ (006)
    (006) —𝜀—[Field(fn_body)]→ (009)
    (009) —{↗¹}—𝜀—[EndObject]→ (✓)
    (010) —(parent)→ (011)
    (011) —{↘.}—(first_child)—[CaptureNode]→ (012)
    (012) —{↗¹}—𝜀→ (✓)
    (013) —(parent)→ (014)
    (014) —{↘}—(last_child)—[CaptureNode]→ (016)
    (015) —{↗·¹}—𝜀→ (✓)
    (016) —𝜀→ (015), (017)
    (017) —{→}—(last_child)—[CaptureNode]→ (016)
    (018) —(parent)—[StartObject]→ (019)
    (019) —{↘}—(a)—[CaptureNode]→ (020)
    (020) —𝜀—[Field(left)]→ (021)
    (021) —{→·}—(b)—[CaptureNode]→ (022)
    (022) —𝜀—[Field(right)]→ (025)
    (025) —{↗¹}—𝜀—[EndObject]→ (✓)
    (026) —(a)→ (027)
    (027) —{↘}—(b)→ (028)
    (028) —{↘}—(c)→ (029)
    (029) —{↘}—(d)—[CaptureNode]→ (032)
    (032) —{↗³}—𝜀→ (✓)
    (033) —(container)→ (038)
    (034) —{↘}—(item)—[CaptureNode]→ (040)
    (036) —𝜀—[EndArray]→ (041)
    (038) —𝜀—[StartArray]→ (034), (036)
    (039) —{→}—(item)—[CaptureNode]→ (040)
    (040) —𝜀—[PushElement]→ (039), (036)
    (041) —{↗¹}—𝜀→ (✓)
    (042) —(container)→ (044)
    (043) —{↘}—(item)—[CaptureNode]→ (049)
    (044) —𝜀—[StartArray]→ (043)
    (045) —𝜀—[EndArray]→ (050)
    (047) —𝜀→ (✓)
    (048) —{→}—(item)—[CaptureNode]→ (049)
    (049) —𝜀—[PushElement]→ (048), (045)
    (050) —{↗¹}—𝜀→ (✓)
    (051) —(container)→ (053)
    (052) —(item)—[CaptureNode]→ (056)
    (053) —𝜀→ (052), (055)
    (055) —𝜀—[ClearCurrent]→ (056)
    (056) —{↗¹}—𝜀→ (✓)
    (057) —(function)—[StartObject]→ (058)
    (058) —{↘}—(identifier)@name—[CaptureNode]→ (059)
    (059) —𝜀—[Field(name)]→ (060)
    (060) —{→}—(block)@body—[CaptureNode]→ (061)
    (061) —𝜀—[Field(body)]→ (066)
    (066) —{↗¹}—𝜀—[EndObject]→ (072)
    (068) —𝜀—[StartObject, StartArray]→ (057), (074)
    (069) —{→}—(function)→ (058), (071)
    (070) —𝜀—[StartObject]→ (069)
    (071) —𝜀—[EndObject]→ (072)
    (072) —𝜀—[PushElement]→ (070), (074)
    (074) —𝜀—[EndArray, EndObject]→ (✓)
    (075) —𝜀—[StartObject]→ (076)
    (076) —{→}—(key)—[CaptureNode]→ (077)
    (077) —𝜀—[Field(key)]→ (078)
    (078) —{→}—(value)—[CaptureNode]→ (083)
    (083) —𝜀—[Field(value), EndObject]→ (089)
    (085) —𝜀—[StartObject, StartArray]→ (075), (091)
    (086) —{→}—𝜀→ (076), (088)
    (087) —𝜀—[StartObject]→ (086)
    (088) —𝜀—[EndObject]→ (089)
    (089) —𝜀—[PushElement]→ (087), (091)
    (091) —𝜀—[EndArray, EndObject]→ (✓)
    (093) —{→}—(item)—[CaptureNode]→ (099)
    (095) —𝜀—[EndArray]→ (✓)
    (097) —𝜀—[StartArray]→ (093), (095)
    (098) —{→}—𝜀→ (093), (099)
    (099) —𝜀—[PushElement]→ (098), (095)
    (100) —𝜀—[StartObject]→ (103), (107)
    (103) —(success)—[StartVariant(Ok), CaptureNode]→ (105)
    (105) —𝜀—[Field(val), EndVariant]→ (111)
    (107) —(error)—[StartVariant(Err), CaptureNode, ToString]→ (109)
    (109) —𝜀—[Field(msg), EndVariant]→ (111)
    (111) —𝜀—[EndObject]→ (✓)
    (112) —(wrapper)→ (123)
    (113) —{↘}—𝜀→ (116), (120)
    (116) —(left_node)—[StartVariant(Left), CaptureNode]→ (118)
    (118) —𝜀—[Field(l), EndVariant]→ (124)
    (120) —(right_node)—[StartVariant(Right), CaptureNode]→ (122)
    (122) —𝜀—[Field(r), EndVariant]→ (124)
    (123) —𝜀—[StartObject]→ (113)
    (124) —𝜀—[EndObject]→ (125)
    (125) —{↗¹}—𝜀→ (✓)
    (126) —𝜀—[StartObject]→ (129), (133)
    (129) —(node)—[StartVariant(Simple), CaptureNode]→ (131)
    (131) —𝜀—[Field(val), EndVariant]→ (141)
    (133) —(pair)—[StartVariant(Complex), StartObject]→ (134)
    (134) —{↘}—(key)—[CaptureNode]→ (135)
    (135) —𝜀—[Field(k)]→ (136)
    (136) —{→}—(value)—[CaptureNode]→ (137)
    (137) —𝜀—[Field(v)]→ (139)
    (139) —{↗¹}—𝜀—[EndObject, EndVariant]→ (141)
    (141) —𝜀—[EndObject]→ (✓)
    (142) —𝜀—[StartObject]→ (144), (146)
    (144) —(a)—[CaptureNode]→ (145)
    (145) —𝜀—[Field(val)]→ (149)
    (146) —(b)—[CaptureNode]→ (147)
    (147) —𝜀—[Field(val)]→ (149)
    (149) —𝜀—[EndObject]→ (✓)
    (150) —𝜀—[StartObject]→ (152), (154)
    (152) —(a)—[CaptureNode]→ (153)
    (153) —𝜀—[Field(x)]→ (157)
    (154) —(b)—[CaptureNode]→ (155)
    (155) —𝜀—[Field(y)]→ (157)
    (157) —𝜀—[EndObject]→ (✓)
    (158) —𝜀—[StartObject]→ (160), (162)
    (160) —(a)—[CaptureNode]→ (161)
    (161) —𝜀—[Field(x)]→ (165)
    (162) —(b)—[CaptureNode]→ (163)
    (163) —𝜀—[Field(y)]→ (165)
    (165) —𝜀—[EndObject]→ (✓)
    (166) —(outer)→ (172)
    (167) —{↘}—𝜀→ (168)
    (168) —{→}—(inner)—[CaptureNode]→ (169)
    (169) —𝜀—[Field(x)]→ (170)
    (170) —{→}—(inner2)—[CaptureNode]→ (173)
    (172) —𝜀—[StartObject]→ (167)
    (173) —𝜀—[Field(y), EndObject]→ (174)
    (174) —{↗¹}—𝜀→ (✓)
    (175) —(outer)—[StartObject]→ (176)
    (176) —{↘}—𝜀→ (177)
    (177) —{→}—(inner)—[CaptureNode]→ (178)
    (178) —𝜀—[Field(x)]→ (179)
    (179) —{→}—(inner2)—[CaptureNode]→ (180)
    (180) —𝜀—[Field(y)]→ (183)
    (183) —{↗¹}—𝜀—[EndObject]→ (✓)
    (185) —{→}—𝜀→ (186)
    (186) —{→}—(a)—[CaptureNode]→ (194)
    (188) —𝜀—[StartObject, StartObject]→ (185)
    (191) —{→}—𝜀→ (192)
    (192) —{→}—(b)—[CaptureNode]→ (198)
    (194) —𝜀—[Field(a), EndObject, Field(inner1), StartObject]→ (191)
    (198) —𝜀—[Field(b), EndObject, Field(inner2), EndObject]→ (✓)
    (199) —(identifier)—[CaptureNode]→ (✓)
    (200) —<Identifier>—𝜀→ (199), (201)
    (201) —𝜀—<Identifier>→ (✓)
    (202) —<Identifier>—𝜀→ (199), (203)
    (203) —𝜀—<Identifier>—[CaptureNode]→ (✓)
    (204) —<RefSimple>—𝜀→ (200), (205)
    (205) —𝜀—<RefSimple>→ (✓)
    (206) —𝜀—[StartObject]→ (208), (210)
    (208) —(single)—[CaptureNode]→ (209)
    (209) —𝜀—[Field(item)]→ (221)
    (210) —(multi)→ (212)
    (211) —{↘}—(x)—[CaptureNode]→ (217)
    (212) —𝜀—[StartArray]→ (211)
    (215) —𝜀→ (✓)
    (216) —{→}—(x)—[CaptureNode]→ (217)
    (217) —𝜀—[PushElement]→ (216), (218)
    (218) —𝜀—[EndArray, Field(item)]→ (219)
    (219) —{↗¹}—𝜀→ (221)
    (221) —𝜀—[EndObject]→ (✓)
    (222) —(_)—[StartArray, StartObject, CaptureNode]→ (227)
    (223) —{↘}—(item)—[CaptureNode, CaptureNode]→ (229)
    (227) —𝜀—[StartArray]→ (223), (230)
    (228) —{→}—(item)—[CaptureNode, CaptureNode]→ (229)
    (229) —𝜀—[PushElement]→ (228), (230)
    (230) —𝜀—[EndArray, Field(inner)]→ (235)
    (233) —𝜀—[EndArray]→ (✓)
    (235) —{↗¹}—𝜀—[EndObject]→ (241)
    (237) —𝜀→ (✓)
    (238) —{→}—(_)—[CaptureNode]→ (227), (240)
    (239) —𝜀—[StartObject]→ (238)
    (240) —𝜀—[EndObject]→ (241)
    (241) —𝜀—[PushElement]→ (239), (233)
    (242) —(module)—[StartObject]→ (243)
    (243) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (249)
    (245) —{→·}—(import)—[CaptureNode]→ (251)
    (249) —𝜀—[Field(mod_name), StartArray]→ (245), (252)
    (250) —{→}—(import)—[CaptureNode]→ (251)
    (251) —𝜀—[PushElement]→ (250), (252)
    (252) —𝜀—[EndArray, Field(imports)]→ (253)
    (253) —{→}—(block)@body→ (294)
    (254) —{↘}—𝜀→ (255)
    (255) —{→}—𝜀→ (258), (282)
    (258) —(function)—[StartVariant(Func), StartObject, CaptureNode]→ (259)
    (259) —{↘}—(identifier)@name—[CaptureNode, ToString, CaptureNode]→ (260)
    (260) —𝜀—[Field(fn_name)]→ (261)
    (261) —{→}—(parameters)@params—[CaptureNode]→ (270)
    (262) —{↘}—𝜀→ (263)
    (263) —{→}—(param)—[CaptureNode, CaptureNode, CaptureNode]→ (268)
    (267) —𝜀—[StartObject]→ (262)
    (268) —𝜀—[Field(p), EndObject]→ (274)
    (270) —𝜀—[StartArray]→ (267), (275)
    (271) —{→}—𝜀→ (263), (273)
    (272) —𝜀—[StartObject]→ (271)
    (273) —𝜀—[EndObject]→ (274)
    (274) —𝜀—[PushElement]→ (272), (275)
    (275) —𝜀—[EndArray, Field(params)]→ (276)
    (276) —{↗¹}—𝜀→ (277)
    (277) —{→}—(block)@body—[CaptureNode, CaptureNode]→ (278)
    (278) —𝜀—[Field(fn_body)]→ (280)
    (280) —{↗¹}—𝜀—[EndObject, EndVariant]→ (292)
    (282) —(class)—[StartVariant(Class), StartObject, CaptureNode]→ (283)
    (283) —{↘}—(identifier)@name—[CaptureNode, ToString, CaptureNode]→ (284)
    (284) —𝜀—[Field(cls_name)]→ (285)
    (285) —{→}—(class_body)@body—[CaptureNode, CaptureNode]→ (286)
    (286) —𝜀—[Field(cls_body)]→ (288)
    (288) —{↗¹}—𝜀—[EndObject, EndVariant]→ (292)
    (291) —𝜀—[StartObject]→ (254)
    (292) —𝜀—[EndObject]→ (298)
    (294) —𝜀—[StartArray]→ (291), (299)
    (295) —{→}—𝜀→ (255), (297)
    (296) —𝜀—[StartObject]→ (295)
    (297) —𝜀—[EndObject]→ (298)
    (298) —𝜀—[PushElement]→ (296), (299)
    (299) —𝜀—[EndArray, Field(items)]→ (300)
    (300) —{↗¹}—𝜀→ (302)
    (302) —𝜀→ (305), (303)
    (303) —{→}—(block)@body→ (302)
    (305) —{↗·¹}—𝜀—[EndObject]→ (✓)
    (306) —(🞵)—[CaptureNode]→ (✓)
    (307) —"+"—[CaptureNode]→ (✓)
    (308) —(identifier)→ (✓)
    (309) —𝜀→ (312), (315)
    (310) —𝜀→ (✓)
    (312) —(value)—[StartVariant(Some), CaptureNode]→ (313)
    (313) —𝜀—[EndVariant]→ (310)
    (315) —(none_marker)—[StartVariant(None)]→ (316)
    (316) —𝜀—[EndVariant]→ (310)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    Identifier = Node
    RefSimple = ()
    WildcardCapture = Node
    UntaggedSymmetric = Node
    UntaggedCaptured = UntaggedCapturedScope3
    TaggedCaptured = TaggedCapturedScope13
    StringLiteral = Node
    StringCapture = str
    StarQuant = [Node]
    SimpleCapture = Node
    RefChain = ()
    RefCaptured = Node
    QisSequence = T16
    QisNode = T18
    PlusQuant = [Node]⁺
    OptQuant = Node?
    NoQis = [Node]
    NoCaptures = ()
    NestedScopes = NestedScopesScope24
    NestedQuant = T27
    DeepNest = Node
    CardinalityJoin = [Node]⁺
    CapturedSeq = CapturedSeqScope42
    AnchorLast = Node
    AnchorFirst = Node

    UntaggedCapturedScope3 = {
      x: Node?
      y: Node?
    }
    UntaggedAsymmetric = {
      x: Node?
      y: Node?
    }
    UncapturedSeq = {
      x: Node
      y: Node
    }
    TaggedRoot = {
      Ok => Node
      Err => str
    }
    TaggedMultiScope11 = {
      k: Node
      v: Node
    }
    TaggedMulti = {
      Simple => Node
      Complex => TaggedMultiScope11
    }
    TaggedCapturedScope13 = {
      Left => Node
      Right => Node
    }
    QisSequenceScope15 = {
      key: Node
      value: Node
    }
    T16 = [QisSequenceScope15]
    QisNodeScope17 = {
      name: Node
      body: Node
    }
    T18 = [QisNodeScope17]
    NestedScopesScope22 = { a: Node }
    NestedScopesScope23 = { b: Node }
    NestedScopesScope24 = {
      inner1: NestedScopesScope22
      inner2: NestedScopesScope23
    }
    NestedQuantScope25 = { inner: [Node] }
    T27 = [NestedQuantScope25]⁺
    MultiCapture = {
      fn_name: str
      fn_body: Node
    }
    EmptyBranch = {
      Some => Node
      None => ()
    }
    ComplexScope30 = { p: Node }
    T31 = [ComplexScope30]
    T33 = T31?
    ComplexScope32 = {
      fn_name: str?
      params: T33
      fn_body: Node?
      cls_name: str?
      cls_body: Node?
    }
    T38 = [ComplexScope32]
    Complex = {
      mod_name: str
      imports: [Node]
      items: T38
    }
    CapturedSeqScope42 = {
      x: Node
      y: Node
    }
    AnchorSibling = {
      left: Node
      right: Node
    }
    "#);
}

/// Test specifically for ADR-0008 navigation lowering.
#[test]
fn golden_navigation_patterns() {
    let source = indoc! {r#"
        // Stay - first transition at root
        NavStay = (root) @r

        // Down - descend to children (skip any)
        NavDown = (parent (child) @c)

        // DownSkipTrivia - anchor at first child
        NavDownAnchor = (parent . (child) @c)

        // Next - sibling traversal (skip any)
        NavNext = (parent (a) @a (b) @b)

        // NextSkipTrivia - adjacent siblings
        NavNextAnchor = (parent (a) @a . (b) @b)

        // Up - ascend (no constraint)
        NavUp = (a (b (c) @c))

        // UpSkipTrivia - must be last non-trivia
        NavUpAnchor = (parent (child) @c .)

        // Multi-level Up
        NavUpMulti = (a (b (c (d (e) @e))))

        // Mixed anchors
        NavMixed = (outer . (first) @f (middle) @m . (last) @l .)
    "#};

    insta::assert_snapshot!(golden_master(source), @r"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    NavStay = (00)
    NavDown = (01)
    NavDownAnchor = (04)
    NavNext = (07)
    NavNextAnchor = (15)
    NavUp = (23)
    NavUpAnchor = (28)
    NavUpMulti = (33)
    NavMixed = (42)

    (00) —(root)—[CaptureNode]→ (✓)
    (01) —(parent)→ (02)
    (02) —{↘}—(child)—[CaptureNode]→ (03)
    (03) —{↗¹}—𝜀→ (✓)
    (04) —(parent)→ (05)
    (05) —{↘.}—(child)—[CaptureNode]→ (06)
    (06) —{↗¹}—𝜀→ (✓)
    (07) —(parent)—[StartObject]→ (08)
    (08) —{↘}—(a)—[CaptureNode]→ (09)
    (09) —𝜀—[Field(a)]→ (10)
    (10) —{→}—(b)—[CaptureNode]→ (11)
    (11) —𝜀—[Field(b)]→ (14)
    (14) —{↗¹}—𝜀—[EndObject]→ (✓)
    (15) —(parent)—[StartObject]→ (16)
    (16) —{↘}—(a)—[CaptureNode]→ (17)
    (17) —𝜀—[Field(a)]→ (18)
    (18) —{→·}—(b)—[CaptureNode]→ (19)
    (19) —𝜀—[Field(b)]→ (22)
    (22) —{↗¹}—𝜀—[EndObject]→ (✓)
    (23) —(a)→ (24)
    (24) —{↘}—(b)→ (25)
    (25) —{↘}—(c)—[CaptureNode]→ (27)
    (27) —{↗²}—𝜀→ (✓)
    (28) —(parent)→ (29)
    (29) —{↘}—(child)—[CaptureNode]→ (31)
    (30) —{↗·¹}—𝜀→ (✓)
    (31) —𝜀→ (30), (32)
    (32) —{→}—(child)—[CaptureNode]→ (31)
    (33) —(a)→ (34)
    (34) —{↘}—(b)→ (35)
    (35) —{↘}—(c)→ (36)
    (36) —{↘}—(d)→ (37)
    (37) —{↘}—(e)—[CaptureNode]→ (41)
    (41) —{↗⁴}—𝜀→ (✓)
    (42) —(outer)—[StartObject]→ (43)
    (43) —{↘.}—(first)—[CaptureNode]→ (44)
    (44) —𝜀—[Field(f)]→ (45)
    (45) —{→}—(middle)—[CaptureNode]→ (46)
    (46) —𝜀—[Field(m)]→ (47)
    (47) —{→·}—(last)—[CaptureNode]→ (48)
    (48) —𝜀—[Field(l)]→ (50)
    (50) —𝜀→ (53), (51)
    (51) —{→}—(last)—[CaptureNode]→ (50)
    (53) —{↗·¹}—𝜀—[EndObject]→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    NavUpMulti = Node
    NavUpAnchor = Node
    NavUp = Node
    NavStay = Node
    NavDownAnchor = Node
    NavDown = Node

    NavNextAnchor = {
      a: Node
      b: Node
    }
    NavNext = {
      a: Node
      b: Node
    }
    NavMixed = {
      f: Node
      m: Node
      l: Node
    }
    ");
}

/// Test specifically for ADR-0009 type inference edge cases.
#[test]
fn golden_type_inference() {
    let source = indoc! {r#"
        // Flat scoping - nesting doesn't create data nesting
        FlatScope = (a (b (c (d) @val)))

        // Reference opacity - calling doesn't inherit captures
        BaseWithCapture = (identifier) @name
        RefOpaque = (BaseWithCapture)
        RefCaptured = (BaseWithCapture) @result

        // Tagged at root vs inline
        TaggedAtRoot = [ A: (a) @x  B: (b) @y ]
        TaggedInline = (wrapper [ A: (a) @x  B: (b) @y ])

        // Cardinality multiplication
        // outer(*) * inner(+) = *
        CardMult = ((item)+ @items)*

        // QIS vs non-QIS
        QisTwo = { (a) @x (b) @y }*
        NoQisOne = { (a) @x }*

        // Missing field rule - asymmetric → Optional
        MissingField = [
            Full: (full (a) @a (b) @b (c) @c)
            Partial: (partial (a) @a)
        ]

        // Synthetic naming
        SyntheticNames = (foo { (bar) @bar } @baz)
    "#};

    insta::assert_snapshot!(golden_master(source), @r"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    FlatScope = (000)
    BaseWithCapture = (007)
    RefOpaque = (008)
    RefCaptured = (010)
    TaggedAtRoot = (012)
    TaggedInline = (024)
    CardMult = (050)
    QisTwo = (063)
    NoQisOne = (075)
    MissingField = (078)
    SyntheticNames = (098)

    (000) —(a)→ (001)
    (001) —{↘}—(b)→ (002)
    (002) —{↘}—(c)→ (003)
    (003) —{↘}—(d)—[CaptureNode]→ (006)
    (006) —{↗³}—𝜀→ (✓)
    (007) —(identifier)—[CaptureNode]→ (✓)
    (008) —<BaseWithCapture>—𝜀→ (007), (009)
    (009) —𝜀—<BaseWithCapture>→ (✓)
    (010) —<BaseWithCapture>—𝜀→ (007), (011)
    (011) —𝜀—<BaseWithCapture>—[CaptureNode]→ (✓)
    (012) —𝜀—[StartObject]→ (015), (019)
    (015) —(a)—[StartVariant(A), CaptureNode]→ (017)
    (017) —𝜀—[Field(x), EndVariant]→ (023)
    (019) —(b)—[StartVariant(B), CaptureNode]→ (021)
    (021) —𝜀—[Field(y), EndVariant]→ (023)
    (023) —𝜀—[EndObject]→ (✓)
    (024) —(wrapper)—[StartObject]→ (025)
    (025) —{↘}—𝜀→ (028), (032)
    (028) —(a)—[StartVariant(A), CaptureNode]→ (030)
    (030) —𝜀—[Field(x), EndVariant]→ (037)
    (032) —(b)—[StartVariant(B), CaptureNode]→ (034)
    (034) —𝜀—[Field(y), EndVariant]→ (037)
    (037) —{↗¹}—𝜀—[EndObject]→ (✓)
    (038) —(_)→ (040)
    (039) —{↘}—(item)—[CaptureNode]→ (045)
    (040) —𝜀—[StartArray]→ (039)
    (041) —𝜀—[EndArray]→ (046)
    (043) —𝜀→ (✓)
    (044) —{→}—(item)—[CaptureNode]→ (045)
    (045) —𝜀—[PushElement]→ (044), (041)
    (046) —{↗¹}—𝜀→ (052)
    (048) —𝜀—[EndArray]→ (✓)
    (050) —𝜀—[StartArray]→ (038), (048)
    (051) —{→}—(_)→ (040), (052)
    (052) —𝜀—[PushElement]→ (051), (048)
    (053) —𝜀—[StartObject]→ (054)
    (054) —{→}—(a)—[CaptureNode]→ (055)
    (055) —𝜀—[Field(x)]→ (056)
    (056) —{→}—(b)—[CaptureNode]→ (061)
    (061) —𝜀—[Field(y), EndObject]→ (067)
    (063) —𝜀—[StartObject, StartArray]→ (053), (069)
    (064) —{→}—𝜀→ (054), (066)
    (065) —𝜀—[StartObject]→ (064)
    (066) —𝜀—[EndObject]→ (067)
    (067) —𝜀—[PushElement]→ (065), (069)
    (069) —𝜀—[EndArray, EndObject]→ (✓)
    (071) —{→}—(a)—[CaptureNode]→ (077)
    (073) —𝜀—[EndArray]→ (✓)
    (075) —𝜀—[StartArray]→ (071), (073)
    (076) —{→}—𝜀→ (071), (077)
    (077) —𝜀—[PushElement]→ (076), (073)
    (078) —𝜀—[StartObject]→ (081), (091)
    (081) —(full)—[StartVariant(Full), StartObject]→ (082)
    (082) —{↘}—(a)—[CaptureNode]→ (083)
    (083) —𝜀—[Field(a)]→ (084)
    (084) —{→}—(b)—[CaptureNode]→ (085)
    (085) —𝜀—[Field(b)]→ (086)
    (086) —{→}—(c)—[CaptureNode]→ (087)
    (087) —𝜀—[Field(c)]→ (089)
    (089) —{↗¹}—𝜀—[EndObject, EndVariant]→ (097)
    (091) —(partial)—[StartVariant(Partial)]→ (092)
    (092) —{↘}—(a)—[CaptureNode]→ (093)
    (093) —𝜀—[Field(a)]→ (095)
    (095) —{↗¹}—𝜀—[EndVariant]→ (097)
    (097) —𝜀—[EndObject]→ (✓)
    (098) —(foo)→ (102)
    (099) —{↘}—𝜀→ (100)
    (100) —{→}—(bar)—[CaptureNode]→ (103)
    (102) —𝜀—[StartObject]→ (099)
    (103) —𝜀—[Field(bar), EndObject]→ (104)
    (104) —{↗¹}—𝜀→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    BaseWithCapture = Node
    SyntheticNames = SyntheticNamesScope7
    RefOpaque = ()
    RefCaptured = Node
    QisTwo = T09
    NoQisOne = [Node]
    FlatScope = Node
    CardMult = [Node]

    TaggedInline = {
      x: Node?
      y: Node?
    }
    TaggedAtRoot = {
      A => Node
      B => Node
    }
    SyntheticNamesScope7 = { bar: Node }
    QisTwoScope8 = {
      x: Node
      y: Node
    }
    T09 = [QisTwoScope8]
    MissingFieldScope11 = {
      a: Node
      b: Node
      c: Node
    }
    MissingField = {
      Full => MissingFieldScope11
      Partial => Node
    }
    ");
}

/// Test ADR-0005 effect stream patterns.
#[test]
fn golden_effect_patterns() {
    let source = indoc! {r#"
        // CaptureNode + Field
        EffCapture = (node) @name

        // ToString
        EffToString = (node) @name ::string

        // StartArray / Push / EndArray
        EffArray = (container (item)* @items)

        // StartObject / Field / EndObject (via captured sequence)
        EffObject = { (a) @x (b) @y } @obj

        // StartVariant / EndVariant (via tagged alternation)
        EffVariant = [ A: (a) @x  B: (b) @y ] @choice

        // Clear (via optional skip path)
        EffClear = (container (item)? @maybe)
    "#};

    insta::assert_snapshot!(golden_master(source), @r"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    EffCapture = (00)
    EffToString = (01)
    EffArray = (02)
    EffObject = (11)
    EffVariant = (18)
    EffClear = (30)

    (00) —(node)—[CaptureNode]→ (✓)
    (01) —(node)—[CaptureNode, ToString]→ (✓)
    (02) —(container)→ (07)
    (03) —{↘}—(item)—[CaptureNode]→ (09)
    (05) —𝜀—[EndArray]→ (10)
    (07) —𝜀—[StartArray]→ (03), (05)
    (08) —{→}—(item)—[CaptureNode]→ (09)
    (09) —𝜀—[PushElement]→ (08), (05)
    (10) —{↗¹}—𝜀→ (✓)
    (11) —𝜀—[StartObject]→ (12)
    (12) —{→}—(a)—[CaptureNode]→ (13)
    (13) —𝜀—[Field(x)]→ (14)
    (14) —{→}—(b)—[CaptureNode]→ (17)
    (17) —𝜀—[Field(y), EndObject]→ (✓)
    (18) —𝜀—[StartObject]→ (21), (25)
    (21) —(a)—[StartVariant(A), CaptureNode]→ (23)
    (23) —𝜀—[Field(x), EndVariant]→ (29)
    (25) —(b)—[StartVariant(B), CaptureNode]→ (27)
    (27) —𝜀—[Field(y), EndVariant]→ (29)
    (29) —𝜀—[EndObject]→ (✓)
    (30) —(container)→ (32)
    (31) —(item)—[CaptureNode]→ (35)
    (32) —𝜀→ (31), (34)
    (34) —𝜀—[ClearCurrent]→ (35)
    (35) —{↗¹}—𝜀→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    EffVariant = EffVariantScope3
    EffToString = str
    EffObject = EffObjectScope4
    EffClear = Node?
    EffCapture = Node
    EffArray = [Node]

    EffVariantScope3 = {
      A => Node
      B => Node
    }
    EffObjectScope4 = {
      x: Node
      y: Node
    }
    ");
}

/// Test quantifier graph structure (ADR-0005).
#[test]
fn golden_quantifier_graphs() {
    let source = indoc! {r#"
        // Greedy star: Branch.next = [match, exit]
        GreedyStar = (a)* @items

        // Greedy plus: must match at least once
        GreedyPlus = (a)+ @items

        // Optional: branch to match or skip
        Optional = (a)? @maybe

        // Non-greedy star: Branch.next = [exit, match]
        LazyStar = (a)*? @items

        // Non-greedy plus
        LazyPlus = (a)+? @items

        // Quantifier on sequence (QIS triggered)
        QuantSeq = { (a) @x (b) @y }*

        // Nested quantifiers
        NestedQuant = (outer (inner)* @inners)+ @outers
    "#};

    insta::assert_snapshot!(golden_master(source), @r"
    ═══════════════════════════════════════════════════════════════════════════════
                                  TRANSITION GRAPH
    ═══════════════════════════════════════════════════════════════════════════════

    GreedyStar = (04)
    GreedyPlus = (07)
    Optional = (15)
    LazyStar = (22)
    LazyPlus = (25)
    QuantSeq = (42)
    NestedQuant = (49)

    (00) —(a)—[CaptureNode]→ (06)
    (02) —𝜀—[EndArray]→ (✓)
    (04) —𝜀—[StartArray]→ (00), (02)
    (05) —{→}—(a)—[CaptureNode]→ (06)
    (06) —𝜀—[PushElement]→ (05), (02)
    (07) —(a)—[StartArray, CaptureNode]→ (13)
    (09) —𝜀—[EndArray]→ (✓)
    (11) —𝜀→ (✓)
    (12) —{→}—(a)—[CaptureNode]→ (13)
    (13) —𝜀—[PushElement]→ (12), (09)
    (14) —(a)—[CaptureNode]→ (16)
    (15) —𝜀→ (14), (17)
    (16) —𝜀→ (✓)
    (17) —𝜀—[ClearCurrent]→ (16)
    (18) —(a)—[CaptureNode]→ (24)
    (20) —𝜀—[EndArray]→ (✓)
    (22) —𝜀—[StartArray]→ (20), (18)
    (23) —{→}—(a)—[CaptureNode]→ (24)
    (24) —𝜀—[PushElement]→ (20), (23)
    (25) —(a)—[StartArray, CaptureNode]→ (31)
    (27) —𝜀—[EndArray]→ (✓)
    (29) —𝜀→ (✓)
    (30) —{→}—(a)—[CaptureNode]→ (31)
    (31) —𝜀—[PushElement]→ (27), (30)
    (32) —𝜀—[StartObject]→ (33)
    (33) —{→}—(a)—[CaptureNode]→ (34)
    (34) —𝜀—[Field(x)]→ (35)
    (35) —{→}—(b)—[CaptureNode]→ (40)
    (40) —𝜀—[Field(y), EndObject]→ (46)
    (42) —𝜀—[StartObject, StartArray]→ (32), (48)
    (43) —{→}—𝜀→ (33), (45)
    (44) —𝜀—[StartObject]→ (43)
    (45) —𝜀—[EndObject]→ (46)
    (46) —𝜀—[PushElement]→ (44), (48)
    (48) —𝜀—[EndArray, EndObject]→ (✓)
    (49) —(outer)—[StartArray, StartObject, CaptureNode]→ (54)
    (50) —{↘}—(inner)—[CaptureNode, CaptureNode]→ (56)
    (54) —𝜀—[StartArray]→ (50), (57)
    (55) —{→}—(inner)—[CaptureNode, CaptureNode]→ (56)
    (56) —𝜀—[PushElement]→ (55), (57)
    (57) —𝜀—[EndArray, Field(inners)]→ (62)
    (60) —𝜀—[EndArray]→ (✓)
    (62) —{↗¹}—𝜀—[EndObject]→ (68)
    (64) —𝜀→ (✓)
    (65) —{→}—(outer)—[CaptureNode]→ (54), (67)
    (66) —𝜀—[StartObject]→ (65)
    (67) —𝜀—[EndObject]→ (68)
    (68) —𝜀—[PushElement]→ (66), (60)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    QuantSeq = T04
    Optional = Node?
    NestedQuant = T08
    LazyStar = [Node]
    LazyPlus = [Node]⁺
    GreedyStar = [Node]
    GreedyPlus = [Node]⁺

    QuantSeqScope3 = {
      x: Node
      y: Node
    }
    T04 = [QuantSeqScope3]
    NestedQuantScope6 = { inners: [Node] }
    T08 = [NestedQuantScope6]⁺
    ");
}
