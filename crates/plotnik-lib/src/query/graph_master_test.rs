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
    AnchorSibling = (016)
    DeepNest = (024)
    StarQuant = (031)
    PlusQuant = (041)
    OptQuant = (051)
    QisNode = (068)
    QisSequence = (084)
    NoQis = (095)
    TaggedRoot = (099)
    TaggedCaptured = (111)
    TaggedMulti = (123)
    UntaggedSymmetric = (139)
    UntaggedAsymmetric = (147)
    UntaggedCaptured = (155)
    CapturedSeq = (161)
    UncapturedSeq = (168)
    NestedScopes = (181)
    Identifier = (190)
    RefSimple = (191)
    RefCaptured = (193)
    RefChain = (195)
    CardinalityJoin = (197)
    NestedQuant = (225)
    Complex = (234)
    WildcardCapture = (295)
    StringLiteral = (296)
    NoCaptures = (297)
    EmptyBranch = (298)

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
    (014) —{↘}—(last_child)—[CaptureNode]→ (015)
    (015) —{↗·¹}—𝜀→ (✓)
    (016) —(parent)—[StartObject]→ (017)
    (017) —{↘}—(a)—[CaptureNode]→ (018)
    (018) —𝜀—[Field(left)]→ (019)
    (019) —{→·}—(b)—[CaptureNode]→ (020)
    (020) —𝜀—[Field(right)]→ (023)
    (023) —{↗¹}—𝜀—[EndObject]→ (✓)
    (024) —(a)→ (025)
    (025) —{↘}—(b)→ (026)
    (026) —{↘}—(c)→ (027)
    (027) —{↘}—(d)—[CaptureNode]→ (030)
    (030) —{↗³}—𝜀→ (✓)
    (031) —(container)→ (036)
    (032) —(item)—[CaptureNode]→ (039)
    (034) —𝜀—[EndArray]→ (040)
    (036) —𝜀—[StartArray]→ (037), (034)
    (037) —{↘}—𝜀→ (032)
    (038) —{→}—𝜀→ (032)
    (039) —𝜀—[PushElement]→ (038), (034)
    (040) —{↗¹}—𝜀→ (✓)
    (041) —(container)→ (043)
    (042) —(item)—[CaptureNode]→ (049)
    (043) —𝜀—[StartArray]→ (047)
    (044) —𝜀—[EndArray]→ (050)
    (046) —𝜀→ (✓)
    (047) —{↘}—𝜀→ (042)
    (048) —{→}—𝜀→ (042)
    (049) —𝜀—[PushElement]→ (048), (044)
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
    (061) —𝜀—[Field(body)]→ (071)
    (068) —𝜀—[StartObject, StartArray]→ (057), (073)
    (070) —{→}—𝜀→ (057)
    (071) —{↗¹}—𝜀—[EndObject, PushElement]→ (070), (073)
    (073) —𝜀—[EndArray, EndObject]→ (✓)
    (074) —𝜀—[StartObject]→ (075)
    (075) —{→}—(key)—[CaptureNode]→ (076)
    (076) —𝜀—[Field(key)]→ (077)
    (077) —{→}—(value)—[CaptureNode]→ (087)
    (084) —𝜀—[StartObject, StartArray]→ (074), (089)
    (086) —{→}—𝜀→ (074)
    (087) —𝜀—[Field(value), EndObject, PushElement]→ (086), (089)
    (089) —𝜀—[EndArray, EndObject]→ (✓)
    (091) —{→}—(item)—[CaptureNode]→ (098)
    (093) —𝜀—[EndArray]→ (✓)
    (095) —𝜀—[StartArray]→ (091), (093)
    (097) —{→}—𝜀→ (091)
    (098) —𝜀—[PushElement]→ (097), (093)
    (099) —𝜀—[StartObject]→ (102), (106)
    (102) —(success)—[StartVariant(Ok), CaptureNode]→ (104)
    (104) —𝜀—[Field(val), EndVariant]→ (110)
    (106) —(error)—[StartVariant(Err), CaptureNode, ToString]→ (108)
    (108) —𝜀—[Field(msg), EndVariant]→ (110)
    (110) —𝜀—[EndObject]→ (✓)
    (111) —(wrapper)→ (112)
    (112) —{↘}—𝜀→ (115), (119)
    (115) —(left_node)—[StartVariant(Left), CaptureNode, CaptureNode]→ (117)
    (117) —𝜀—[Field(l), EndVariant]→ (122)
    (119) —(right_node)—[StartVariant(Right), CaptureNode, CaptureNode]→ (121)
    (121) —𝜀—[Field(r), EndVariant]→ (122)
    (122) —{↗¹}—𝜀→ (✓)
    (123) —𝜀—[StartObject]→ (126), (130)
    (126) —(node)—[StartVariant(Simple), CaptureNode]→ (128)
    (128) —𝜀—[Field(val), EndVariant]→ (138)
    (130) —(pair)—[StartVariant(Complex), StartObject]→ (131)
    (131) —{↘}—(key)—[CaptureNode]→ (132)
    (132) —𝜀—[Field(k)]→ (133)
    (133) —{→}—(value)—[CaptureNode]→ (134)
    (134) —𝜀—[Field(v)]→ (136)
    (136) —{↗¹}—𝜀—[EndObject, EndVariant]→ (138)
    (138) —𝜀—[EndObject]→ (✓)
    (139) —𝜀—[StartObject]→ (141), (143)
    (141) —(a)—[CaptureNode]→ (142)
    (142) —𝜀—[Field(val)]→ (146)
    (143) —(b)—[CaptureNode]→ (144)
    (144) —𝜀—[Field(val)]→ (146)
    (146) —𝜀—[EndObject]→ (✓)
    (147) —𝜀—[StartObject]→ (149), (151)
    (149) —(a)—[CaptureNode]→ (150)
    (150) —𝜀—[Field(x)]→ (154)
    (151) —(b)—[CaptureNode]→ (152)
    (152) —𝜀—[Field(y)]→ (154)
    (154) —𝜀—[EndObject]→ (✓)
    (155) —𝜀→ (157), (159)
    (156) —𝜀→ (✓)
    (157) —(a)—[CaptureNode, CaptureNode]→ (158)
    (158) —𝜀—[Field(x)]→ (156)
    (159) —(b)—[CaptureNode, CaptureNode]→ (160)
    (160) —𝜀—[Field(y)]→ (156)
    (161) —(outer)→ (162)
    (162) —{↘}—𝜀→ (163)
    (163) —{→}—(inner)—[CaptureNode, CaptureNode]→ (164)
    (164) —𝜀—[Field(x)]→ (165)
    (165) —{→}—(inner2)—[CaptureNode]→ (166)
    (166) —𝜀—[Field(y)]→ (167)
    (167) —{↗¹}—𝜀→ (✓)
    (168) —(outer)—[StartObject]→ (169)
    (169) —{↘}—𝜀→ (170)
    (170) —{→}—(inner)—[CaptureNode]→ (171)
    (171) —𝜀—[Field(x)]→ (172)
    (172) —{→}—(inner2)—[CaptureNode]→ (173)
    (173) —𝜀—[Field(y)]→ (176)
    (176) —{↗¹}—𝜀—[EndObject]→ (✓)
    (178) —{→}—𝜀→ (179)
    (179) —{→}—(a)—[CaptureNode, CaptureNode, CaptureNode]→ (187)
    (181) —𝜀—[StartObject]→ (178)
    (184) —{→}—𝜀→ (185)
    (185) —{→}—(b)—[CaptureNode, CaptureNode]→ (189)
    (187) —𝜀—[Field(a), EndObject, Field(inner1), StartObject]→ (184)
    (189) —𝜀—[Field(b), EndObject, Field(inner2)]→ (✓)
    (190) —(identifier)—[CaptureNode]→ (✓)
    (191) —<Identifier>—𝜀→ (190), (192)
    (192) —𝜀—<Identifier>→ (✓)
    (193) —<Identifier>—𝜀→ (190), (194)
    (194) —𝜀—<Identifier>—[CaptureNode]→ (✓)
    (195) —<RefSimple>—𝜀→ (191), (196)
    (196) —𝜀—<RefSimple>→ (✓)
    (197) —𝜀—[StartObject]→ (199), (201)
    (199) —(single)—[CaptureNode]→ (200)
    (200) —𝜀—[Field(item)]→ (213)
    (201) —(multi)→ (203)
    (202) —(x)—[CaptureNode]→ (209)
    (203) —𝜀—[StartArray]→ (207)
    (206) —𝜀→ (✓)
    (207) —{↘}—𝜀→ (202)
    (208) —{→}—𝜀→ (202)
    (209) —𝜀—[PushElement]→ (208), (210)
    (210) —𝜀—[EndArray, Field(item)]→ (211)
    (211) —{↗¹}—𝜀→ (213)
    (213) —𝜀—[EndObject]→ (✓)
    (214) —(_)—[StartObject, CaptureNode]→ (219)
    (215) —(item)—[CaptureNode]→ (222)
    (219) —𝜀—[StartArray]→ (220), (223)
    (220) —{↘}—𝜀→ (215)
    (221) —{→}—𝜀→ (215)
    (222) —𝜀—[PushElement]→ (221), (223)
    (223) —𝜀—[EndArray, Field(inner)]→ (233)
    (225) —𝜀—[StartArray]→ (214)
    (226) —𝜀—[EndArray]→ (✓)
    (230) —𝜀→ (✓)
    (232) —{→}—𝜀→ (214)
    (233) —{↗¹}—𝜀—[EndObject, PushElement]→ (232), (226)
    (234) —(module)—[StartObject]→ (235)
    (235) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (241)
    (237) —(import)—[CaptureNode]→ (244)
    (241) —𝜀—[Field(mod_name), StartArray]→ (242), (245)
    (242) —{→·}—𝜀→ (237)
    (243) —{→}—𝜀→ (237)
    (244) —𝜀—[PushElement]→ (243), (245)
    (245) —𝜀—[EndArray, Field(imports)]→ (246)
    (246) —{→}—(block)@body→ (286)
    (247) —𝜀—[StartObject]→ (248)
    (248) —{→}—𝜀→ (251), (274)
    (251) —(function)—[StartVariant(Func), StartObject, CaptureNode]→ (252)
    (252) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (253)
    (253) —𝜀—[Field(fn_name)]→ (254)
    (254) —{→}—(parameters)@params→ (263)
    (255) —𝜀—[StartObject]→ (256)
    (256) —{→}—(param)—[CaptureNode, CaptureNode]→ (266)
    (263) —𝜀—[StartArray]→ (264), (267)
    (264) —{↘}—𝜀→ (255)
    (265) —{→}—𝜀→ (255)
    (266) —𝜀—[Field(p), EndObject, PushElement]→ (265), (267)
    (267) —𝜀—[EndArray, Field(params)]→ (268)
    (268) —{↗¹}—𝜀→ (269)
    (269) —{→}—(block)@body—[CaptureNode]→ (270)
    (270) —𝜀—[Field(fn_body)]→ (272)
    (272) —{↗¹}—𝜀—[EndObject, EndVariant]→ (289)
    (274) —(class)—[StartVariant(Class), StartObject, CaptureNode]→ (275)
    (275) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (276)
    (276) —𝜀—[Field(cls_name)]→ (277)
    (277) —{→}—(class_body)@body—[CaptureNode]→ (278)
    (278) —𝜀—[Field(cls_body)]→ (280)
    (280) —{↗¹}—𝜀—[EndObject, EndVariant]→ (289)
    (286) —𝜀—[StartArray]→ (287), (290)
    (287) —{↘}—𝜀→ (247)
    (288) —{→}—𝜀→ (247)
    (289) —𝜀—[EndObject, PushElement]→ (288), (290)
    (290) —𝜀—[EndArray, Field(items)]→ (291)
    (291) —{↗¹}—𝜀→ (294)
    (294) —{↗·¹}—𝜀—[EndObject]→ (✓)
    (295) —(🞵)—[CaptureNode]→ (✓)
    (296) —"+"—[CaptureNode]→ (✓)
    (297) —(identifier)→ (✓)
    (298) —𝜀→ (301), (304)
    (299) —𝜀→ (✓)
    (301) —(value)—[StartVariant(Some), CaptureNode]→ (302)
    (302) —𝜀—[EndVariant]→ (299)
    (304) —(none_marker)—[StartVariant(None)]→ (305)
    (305) —𝜀—[EndVariant]→ (299)

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
    NavUpMulti = (31)
    NavMixed = (40)

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
    (29) —{↘}—(child)—[CaptureNode]→ (30)
    (30) —{↗·¹}—𝜀→ (✓)
    (31) —(a)→ (32)
    (32) —{↘}—(b)→ (33)
    (33) —{↘}—(c)→ (34)
    (34) —{↘}—(d)→ (35)
    (35) —{↘}—(e)—[CaptureNode]→ (39)
    (39) —{↗⁴}—𝜀→ (✓)
    (40) —(outer)—[StartObject]→ (41)
    (41) —{↘.}—(first)—[CaptureNode]→ (42)
    (42) —𝜀—[Field(f)]→ (43)
    (43) —{→}—(middle)—[CaptureNode]→ (44)
    (44) —𝜀—[Field(m)]→ (45)
    (45) —{→·}—(last)—[CaptureNode]→ (46)
    (46) —𝜀—[Field(l)]→ (49)
    (49) —{↗·¹}—𝜀—[EndObject]→ (✓)

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
    CardMult = (051)
    QisTwo = (065)
    NoQisOne = (076)
    MissingField = (080)
    SyntheticNames = (100)

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
    (039) —(item)—[CaptureNode]→ (046)
    (040) —𝜀—[StartArray]→ (044)
    (041) —𝜀—[EndArray]→ (054)
    (043) —𝜀→ (✓)
    (044) —{↘}—𝜀→ (039)
    (045) —{→}—𝜀→ (039)
    (046) —𝜀—[PushElement]→ (045), (041)
    (049) —𝜀—[EndArray]→ (✓)
    (051) —𝜀—[StartArray]→ (038), (049)
    (053) —{→}—𝜀→ (038)
    (054) —{↗¹}—𝜀—[PushElement]→ (053), (049)
    (055) —𝜀—[StartObject]→ (056)
    (056) —{→}—(a)—[CaptureNode]→ (057)
    (057) —𝜀—[Field(x)]→ (058)
    (058) —{→}—(b)—[CaptureNode]→ (068)
    (065) —𝜀—[StartObject, StartArray]→ (055), (070)
    (067) —{→}—𝜀→ (055)
    (068) —𝜀—[Field(y), EndObject, PushElement]→ (067), (070)
    (070) —𝜀—[EndArray, EndObject]→ (✓)
    (072) —{→}—(a)—[CaptureNode]→ (079)
    (074) —𝜀—[EndArray]→ (✓)
    (076) —𝜀—[StartArray]→ (072), (074)
    (078) —{→}—𝜀→ (072)
    (079) —𝜀—[PushElement]→ (078), (074)
    (080) —𝜀—[StartObject]→ (083), (093)
    (083) —(full)—[StartVariant(Full), StartObject]→ (084)
    (084) —{↘}—(a)—[CaptureNode]→ (085)
    (085) —𝜀—[Field(a)]→ (086)
    (086) —{→}—(b)—[CaptureNode]→ (087)
    (087) —𝜀—[Field(b)]→ (088)
    (088) —{→}—(c)—[CaptureNode]→ (089)
    (089) —𝜀—[Field(c)]→ (091)
    (091) —{↗¹}—𝜀—[EndObject, EndVariant]→ (099)
    (093) —(partial)—[StartVariant(Partial)]→ (094)
    (094) —{↘}—(a)—[CaptureNode]→ (095)
    (095) —𝜀—[Field(a)]→ (097)
    (097) —{↗¹}—𝜀—[EndVariant]→ (099)
    (099) —𝜀—[EndObject]→ (✓)
    (100) —(foo)→ (101)
    (101) —{↘}—𝜀→ (102)
    (102) —{→}—(bar)—[CaptureNode, CaptureNode]→ (103)
    (103) —𝜀—[Field(bar)]→ (104)
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
    EffObject = (13)
    EffVariant = (17)
    EffClear = (27)

    (00) —(node)—[CaptureNode]→ (✓)
    (01) —(node)—[CaptureNode, ToString]→ (✓)
    (02) —(container)→ (07)
    (03) —(item)—[CaptureNode]→ (10)
    (05) —𝜀—[EndArray]→ (11)
    (07) —𝜀—[StartArray]→ (08), (05)
    (08) —{↘}—𝜀→ (03)
    (09) —{→}—𝜀→ (03)
    (10) —𝜀—[PushElement]→ (09), (05)
    (11) —{↗¹}—𝜀→ (✓)
    (13) —{→}—(a)—[CaptureNode, CaptureNode]→ (14)
    (14) —𝜀—[Field(x)]→ (15)
    (15) —{→}—(b)—[CaptureNode]→ (16)
    (16) —𝜀—[Field(y)]→ (✓)
    (17) —𝜀→ (20), (24)
    (18) —𝜀→ (✓)
    (20) —(a)—[StartVariant(A), CaptureNode, CaptureNode]→ (22)
    (22) —𝜀—[Field(x), EndVariant]→ (18)
    (24) —(b)—[StartVariant(B), CaptureNode, CaptureNode]→ (26)
    (26) —𝜀—[Field(y), EndVariant]→ (18)
    (27) —(container)→ (29)
    (28) —(item)—[CaptureNode]→ (32)
    (29) —𝜀→ (28), (31)
    (31) —𝜀—[ClearCurrent]→ (32)
    (32) —{↗¹}—𝜀→ (✓)

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
    GreedyPlus = (09)
    Optional = (17)
    LazyStar = (24)
    LazyPlus = (29)
    QuantSeq = (46)
    NestedQuant = (63)

    (00) —(a)—[CaptureNode]→ (07)
    (02) —𝜀—[EndArray]→ (✓)
    (04) —𝜀—[StartArray]→ (00), (02)
    (06) —{→}—𝜀→ (00)
    (07) —𝜀—[PushElement]→ (06), (02)
    (08) —(a)—[CaptureNode]→ (15)
    (09) —𝜀—[StartArray]→ (08)
    (10) —𝜀—[EndArray]→ (✓)
    (12) —𝜀→ (✓)
    (14) —{→}—𝜀→ (08)
    (15) —𝜀—[PushElement]→ (14), (10)
    (16) —(a)—[CaptureNode]→ (18)
    (17) —𝜀→ (16), (19)
    (18) —𝜀→ (✓)
    (19) —𝜀—[ClearCurrent]→ (18)
    (20) —(a)—[CaptureNode]→ (27)
    (22) —𝜀—[EndArray]→ (✓)
    (24) —𝜀—[StartArray]→ (22), (20)
    (26) —{→}—𝜀→ (20)
    (27) —𝜀—[PushElement]→ (22), (26)
    (28) —(a)—[CaptureNode]→ (35)
    (29) —𝜀—[StartArray]→ (28)
    (30) —𝜀—[EndArray]→ (✓)
    (32) —𝜀→ (✓)
    (34) —{→}—𝜀→ (28)
    (35) —𝜀—[PushElement]→ (30), (34)
    (36) —𝜀—[StartObject]→ (37)
    (37) —{→}—(a)—[CaptureNode]→ (38)
    (38) —𝜀—[Field(x)]→ (39)
    (39) —{→}—(b)—[CaptureNode]→ (49)
    (46) —𝜀—[StartObject, StartArray]→ (36), (51)
    (48) —{→}—𝜀→ (36)
    (49) —𝜀—[Field(y), EndObject, PushElement]→ (48), (51)
    (51) —𝜀—[EndArray, EndObject]→ (✓)
    (52) —(outer)—[StartObject, CaptureNode]→ (57)
    (53) —(inner)—[CaptureNode]→ (60)
    (57) —𝜀—[StartArray]→ (58), (61)
    (58) —{↘}—𝜀→ (53)
    (59) —{→}—𝜀→ (53)
    (60) —𝜀—[PushElement]→ (59), (61)
    (61) —𝜀—[EndArray, Field(inners)]→ (71)
    (63) —𝜀—[StartArray]→ (52)
    (64) —𝜀—[EndArray]→ (✓)
    (68) —𝜀→ (✓)
    (70) —{→}—𝜀→ (52)
    (71) —{↗¹}—𝜀—[EndObject, PushElement]→ (70), (64)

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
