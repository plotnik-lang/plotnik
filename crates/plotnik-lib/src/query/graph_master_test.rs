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
    StringCapture = (002)
    MultiCapture = (004)
    AnchorFirst = (010)
    AnchorLast = (014)
    AnchorSibling = (018)
    DeepNest = (024)
    StarQuant = (032)
    PlusQuant = (040)
    OptQuant = (048)
    QisNode = (061)
    QisSequence = (072)
    NoQis = (081)
    TaggedRoot = (085)
    TaggedCaptured = (095)
    TaggedMulti = (110)
    UntaggedSymmetric = (124)
    UntaggedAsymmetric = (130)
    UntaggedCaptured = (136)
    CapturedSeq = (145)
    UncapturedSeq = (155)
    NestedScopes = (166)
    Identifier = (178)
    RefSimple = (180)
    RefCaptured = (182)
    RefChain = (185)
    CardinalityJoin = (187)
    NestedQuant = (207)
    Complex = (212)
    WildcardCapture = (262)
    StringLiteral = (264)
    NoCaptures = (266)
    EmptyBranch = (267)

    (000) —(identifier)—[CaptureNode]→ (001)
    (001) —𝜀—[Field(name)]→ (✓)
    (002) —(identifier)—[CaptureNode, ToString]→ (003)
    (003) —𝜀—[Field(name)]→ (✓)
    (004) —(function)→ (005)
    (005) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (006)
    (006) —𝜀—[Field(fn_name)]→ (007)
    (007) —{→}—(block)@body—[CaptureNode]→ (008)
    (008) —𝜀—[Field(fn_body)]→ (009)
    (009) —{↗¹}—𝜀→ (✓)
    (010) —(parent)→ (011)
    (011) —{↘.}—(first_child)—[CaptureNode]→ (012)
    (012) —𝜀—[Field(first)]→ (013)
    (013) —{↗¹}—𝜀→ (✓)
    (014) —(parent)→ (015)
    (015) —{↘}—(last_child)—[CaptureNode]→ (016)
    (016) —𝜀—[Field(last)]→ (017)
    (017) —{↗·¹}—𝜀→ (✓)
    (018) —(parent)→ (019)
    (019) —{↘}—(a)—[CaptureNode]→ (020)
    (020) —𝜀—[Field(left)]→ (021)
    (021) —{→·}—(b)—[CaptureNode]→ (022)
    (022) —𝜀—[Field(right)]→ (023)
    (023) —{↗¹}—𝜀→ (✓)
    (024) —(a)→ (025)
    (025) —{↘}—(b)→ (026)
    (026) —{↘}—(c)→ (027)
    (027) —{↘}—(d)—[CaptureNode]→ (028)
    (028) —𝜀—[Field(deep)]→ (031)
    (031) —{↗³}—𝜀→ (✓)
    (032) —(container)→ (034)
    (033) —{↘}—(item)—[CaptureNode]→ (036)
    (034) —𝜀—[StartArray]→ (037)
    (036) —𝜀—[PushElement]→ (037)
    (037) —𝜀→ (033), (038)
    (038) —𝜀—[EndArray, Field(items)]→ (039)
    (039) —{↗¹}—𝜀→ (✓)
    (040) —(container)→ (042)
    (041) —{↘}—(item)—[CaptureNode]→ (045)
    (042) —𝜀—[StartArray]→ (041)
    (045) —𝜀—[PushElement]→ (041), (046)
    (046) —𝜀—[EndArray, Field(items)]→ (047)
    (047) —{↗¹}—𝜀→ (✓)
    (048) —(container)→ (050)
    (049) —{↘}—(item)—[CaptureNode]→ (053)
    (050) —𝜀→ (049), (052)
    (052) —𝜀—[ClearCurrent]→ (053)
    (053) —𝜀—[Field(maybe_item)]→ (054)
    (054) —{↗¹}—𝜀→ (✓)
    (055) —(function)—[StartObject]→ (056)
    (056) —{↘}—(identifier)@name—[CaptureNode]→ (057)
    (057) —𝜀—[Field(name)]→ (058)
    (058) —{→}—(block)@body—[CaptureNode]→ (059)
    (059) —𝜀—[Field(body)]→ (065)
    (061) —𝜀—[StartArray]→ (066)
    (062) —𝜀—[EndArray]→ (✓)
    (065) —{↗¹}—𝜀—[EndObject, PushElement]→ (066)
    (066) —𝜀→ (055), (062)
    (067) —𝜀—[StartObject]→ (068)
    (068) —{→}—(key)—[CaptureNode]→ (069)
    (069) —𝜀—[Field(key)]→ (070)
    (070) —{→}—(value)—[CaptureNode]→ (076)
    (072) —𝜀—[StartArray]→ (077)
    (073) —𝜀—[EndArray]→ (✓)
    (076) —𝜀—[Field(value), EndObject, PushElement]→ (077)
    (077) —𝜀→ (067), (073)
    (079) —{→}—(item)—[CaptureNode]→ (083)
    (081) —𝜀—[StartArray]→ (084)
    (082) —𝜀—[EndArray]→ (✓)
    (083) —𝜀—[Field(item), PushElement]→ (084)
    (084) —𝜀→ (079), (082)
    (085) —𝜀→ (088), (092)
    (086) —𝜀→ (✓)
    (088) —(success)—[StartVariant(Ok), CaptureNode]→ (090)
    (090) —𝜀—[Field(val), EndVariant]→ (086)
    (092) —(error)—[StartVariant(Err), CaptureNode, ToString]→ (094)
    (094) —𝜀—[Field(msg), EndVariant]→ (086)
    (095) —(wrapper)→ (106)
    (096) —{↘}—𝜀→ (099), (103)
    (099) —(left_node)—[StartVariant(Left), CaptureNode, CaptureNode]→ (101)
    (101) —𝜀—[Field(l), EndVariant]→ (108)
    (103) —(right_node)—[StartVariant(Right), CaptureNode, CaptureNode]→ (105)
    (105) —𝜀—[Field(r), EndVariant]→ (108)
    (106) —𝜀—[StartObject]→ (096)
    (108) —𝜀—[EndObject, Field(choice)]→ (109)
    (109) —{↗¹}—𝜀→ (✓)
    (110) —𝜀→ (113), (117)
    (111) —𝜀→ (✓)
    (113) —(node)—[StartVariant(Simple), CaptureNode]→ (115)
    (115) —𝜀—[Field(val), EndVariant]→ (111)
    (117) —(pair)—[StartVariant(Complex), StartObject]→ (118)
    (118) —{↘}—(key)—[CaptureNode]→ (119)
    (119) —𝜀—[Field(k)]→ (120)
    (120) —{→}—(value)—[CaptureNode]→ (121)
    (121) —𝜀—[Field(v)]→ (123)
    (123) —{↗¹}—𝜀—[EndObject, EndVariant]→ (111)
    (124) —𝜀→ (126), (128)
    (125) —𝜀→ (✓)
    (126) —(a)—[CaptureNode]→ (127)
    (127) —𝜀—[Field(val)]→ (125)
    (128) —(b)—[CaptureNode]→ (129)
    (129) —𝜀—[Field(val)]→ (125)
    (130) —𝜀→ (132), (134)
    (131) —𝜀→ (✓)
    (132) —(a)—[CaptureNode]→ (133)
    (133) —𝜀—[Field(x)]→ (131)
    (134) —(b)—[CaptureNode]→ (135)
    (135) —𝜀—[Field(y)]→ (131)
    (136) —𝜀—[StartObject]→ (138), (140)
    (138) —(a)—[CaptureNode, CaptureNode]→ (139)
    (139) —𝜀—[Field(x)]→ (144)
    (140) —(b)—[CaptureNode, CaptureNode]→ (141)
    (141) —𝜀—[Field(y)]→ (144)
    (144) —𝜀—[EndObject, Field(data)]→ (✓)
    (145) —(outer)→ (151)
    (146) —{↘}—𝜀→ (147)
    (147) —{→}—(inner)—[CaptureNode, CaptureNode]→ (148)
    (148) —𝜀—[Field(x)]→ (149)
    (149) —{→}—(inner2)—[CaptureNode]→ (153)
    (151) —𝜀—[StartObject]→ (146)
    (153) —𝜀—[Field(y), EndObject, Field(nested)]→ (154)
    (154) —{↗¹}—𝜀→ (✓)
    (155) —(outer)→ (156)
    (156) —{↘}—𝜀→ (157)
    (157) —{→}—(inner)—[CaptureNode]→ (158)
    (158) —𝜀—[Field(x)]→ (159)
    (159) —{→}—(inner2)—[CaptureNode]→ (160)
    (160) —𝜀—[Field(y)]→ (161)
    (161) —{↗¹}—𝜀→ (✓)
    (163) —{→}—𝜀→ (164)
    (164) —{→}—(a)—[CaptureNode, CaptureNode, CaptureNode]→ (172)
    (166) —𝜀—[StartObject, StartObject]→ (163)
    (169) —{→}—𝜀→ (170)
    (170) —{→}—(b)—[CaptureNode, CaptureNode]→ (177)
    (172) —𝜀—[Field(a), EndObject, Field(inner1), StartObject]→ (169)
    (177) —𝜀—[Field(b), EndObject, Field(inner2), EndObject, Field(outer)]→ (✓)
    (178) —(identifier)—[CaptureNode]→ (179)
    (179) —𝜀—[Field(id)]→ (✓)
    (180) —<Identifier>—𝜀→ (178), (181)
    (181) —𝜀—<Identifier>→ (✓)
    (182) —<Identifier>—𝜀→ (178), (183)
    (183) —𝜀—<Identifier>—[CaptureNode]→ (184)
    (184) —𝜀—[Field(captured_id)]→ (✓)
    (185) —<RefSimple>—𝜀→ (180), (186)
    (186) —𝜀—<RefSimple>→ (✓)
    (187) —𝜀→ (189), (191)
    (188) —{↗¹}—𝜀→ (✓)
    (189) —(single)—[CaptureNode]→ (190)
    (190) —𝜀—[Field(item)]→ (188)
    (191) —(multi)→ (193)
    (192) —{↘}—(x)—[CaptureNode]→ (196)
    (193) —𝜀—[StartArray]→ (192)
    (196) —𝜀—[PushElement]→ (192), (197)
    (197) —𝜀—[EndArray, Field(item)]→ (188)
    (199) —(_)—[CaptureNode]→ (201)
    (200) —{↘}—(item)—[CaptureNode]→ (203)
    (201) —𝜀—[StartArray]→ (204)
    (203) —𝜀—[PushElement]→ (204)
    (204) —𝜀→ (200), (205)
    (205) —𝜀—[EndArray, Field(inner)]→ (210)
    (207) —𝜀—[StartArray]→ (199)
    (210) —{↗¹}—𝜀—[PushElement]→ (199), (211)
    (211) —𝜀—[EndArray, Field(outer)]→ (✓)
    (212) —(module)→ (213)
    (213) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (216)
    (215) —{→·}—(import)—[CaptureNode]→ (218)
    (216) —𝜀—[Field(mod_name), StartArray]→ (219)
    (218) —𝜀—[PushElement]→ (219)
    (219) —𝜀→ (215), (220)
    (220) —𝜀—[EndArray, Field(imports)]→ (221)
    (221) —{→}—(block)@body→ (251)
    (222) —{↘}—𝜀→ (223)
    (223) —{→}—𝜀→ (226), (244)
    (226) —(function)—[StartVariant(Func), StartObject, CaptureNode]→ (227)
    (227) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (228)
    (228) —𝜀—[Field(fn_name)]→ (229)
    (229) —{→}—(parameters)@params→ (233)
    (230) —{↘}—𝜀→ (231)
    (231) —{→}—(param)—[CaptureNode, CaptureNode]→ (235)
    (233) —𝜀—[StartArray]→ (236)
    (235) —𝜀—[Field(p), PushElement]→ (236)
    (236) —𝜀→ (230), (237)
    (237) —𝜀—[EndArray, Field(params)]→ (238)
    (238) —{↗¹}—𝜀→ (239)
    (239) —{→}—(block)@body—[CaptureNode]→ (240)
    (240) —𝜀—[Field(fn_body)]→ (242)
    (242) —{↗¹}—𝜀—[EndObject, EndVariant]→ (255)
    (244) —(class)—[StartVariant(Class), StartObject, CaptureNode]→ (245)
    (245) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (246)
    (246) —𝜀—[Field(cls_name)]→ (247)
    (247) —{→}—(class_body)@body—[CaptureNode]→ (248)
    (248) —𝜀—[Field(cls_body)]→ (250)
    (250) —{↗¹}—𝜀—[EndObject, EndVariant]→ (255)
    (251) —𝜀—[StartObject, StartArray]→ (256)
    (253) —𝜀—[StartObject]→ (222)
    (255) —𝜀—[EndObject, PushElement]→ (256)
    (256) —𝜀→ (253), (259)
    (259) —𝜀—[EndArray, EndObject, Field(items)]→ (260)
    (260) —{↗¹}—𝜀→ (261)
    (261) —{↗·¹}—𝜀→ (✓)
    (262) —(🞵)—[CaptureNode]→ (263)
    (263) —𝜀—[Field(any)]→ (✓)
    (264) —"+"—[CaptureNode]→ (265)
    (265) —𝜀—[Field(op)]→ (✓)
    (266) —(identifier)→ (✓)
    (267) —𝜀→ (270), (274)
    (268) —𝜀→ (✓)
    (270) —(value)—[StartVariant(Some), CaptureNode]→ (272)
    (272) —𝜀—[Field(val), EndVariant]→ (268)
    (274) —(none_marker)—[StartVariant(None)]→ (275)
    (275) —𝜀—[EndVariant]→ (268)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    RefSimple = ()
    RefChain = ()
    QisSequence = T26
    QisNode = T28
    NoCaptures = ()

    Identifier = { id: Node }
    WildcardCapture = { any: Node }
    UntaggedSymmetric = { val: Node }
    UntaggedCapturedScope6 = {
      x: Node?
      y: Node?
    }
    UntaggedCaptured = { data: UntaggedCapturedScope6 }
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
    TaggedMultiScope15 = {
      k: Node
      v: Node
    }
    TaggedMulti = {
      Simple => Node
      Complex => TaggedMultiScope15
    }
    TaggedCapturedScope17 = {
      Left => Node
      Right => Node
    }
    TaggedCaptured = { choice: TaggedCapturedScope17 }
    StringLiteral = { op: Node }
    StringCapture = { name: str }
    StarQuant = { items: [Node] }
    SimpleCapture = { name: Node }
    RefCaptured = { captured_id: Identifier }
    QisSequenceScope25 = {
      key: Node
      value: Node
    }
    T26 = [QisSequenceScope25]
    QisNodeScope27 = {
      name: Node
      body: Node
    }
    T28 = [QisNodeScope27]
    PlusQuant = { items: [Node]⁺ }
    OptQuant = { maybe_item: Node? }
    NoQis = { item: [Node] }
    NestedScopesScope35 = { a: Node }
    NestedScopesScope36 = { b: Node }
    NestedScopesScope37 = {
      inner1: NestedScopesScope35
      inner2: NestedScopesScope36
    }
    NestedScopes = { outer: NestedScopesScope37 }
    NestedQuant = {
      inner: [Node]
      outer: [Node]⁺
    }
    MultiCapture = {
      fn_name: str
      fn_body: Node
    }
    EmptyBranch = {
      Some => Node
      None => ()
    }
    DeepNest = { deep: Node }
    ComplexScope45 = {
      fn_name: str?
      p: [Node]
      params: [Node]
      fn_body: Node?
      cls_name: str?
      cls_body: Node?
    }
    T52 = [ComplexScope45]
    Complex = {
      mod_name: str
      imports: [Node]
      items: T52
    }
    CardinalityJoin = { item: [Node]⁺ }
    CapturedSeqScope57 = {
      x: Node
      y: Node
    }
    CapturedSeq = { nested: CapturedSeqScope57 }
    AnchorSibling = {
      left: Node
      right: Node
    }
    AnchorLast = { last: Node }
    AnchorFirst = { first: Node }
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
    NavDown = (02)
    NavDownAnchor = (06)
    NavNext = (10)
    NavNextAnchor = (16)
    NavUp = (22)
    NavUpAnchor = (28)
    NavUpMulti = (32)
    NavMixed = (42)

    (00) —(root)—[CaptureNode]→ (01)
    (01) —𝜀—[Field(r)]→ (✓)
    (02) —(parent)→ (03)
    (03) —{↘}—(child)—[CaptureNode]→ (04)
    (04) —𝜀—[Field(c)]→ (05)
    (05) —{↗¹}—𝜀→ (✓)
    (06) —(parent)→ (07)
    (07) —{↘.}—(child)—[CaptureNode]→ (08)
    (08) —𝜀—[Field(c)]→ (09)
    (09) —{↗¹}—𝜀→ (✓)
    (10) —(parent)→ (11)
    (11) —{↘}—(a)—[CaptureNode]→ (12)
    (12) —𝜀—[Field(a)]→ (13)
    (13) —{→}—(b)—[CaptureNode]→ (14)
    (14) —𝜀—[Field(b)]→ (15)
    (15) —{↗¹}—𝜀→ (✓)
    (16) —(parent)→ (17)
    (17) —{↘}—(a)—[CaptureNode]→ (18)
    (18) —𝜀—[Field(a)]→ (19)
    (19) —{→·}—(b)—[CaptureNode]→ (20)
    (20) —𝜀—[Field(b)]→ (21)
    (21) —{↗¹}—𝜀→ (✓)
    (22) —(a)→ (23)
    (23) —{↘}—(b)→ (24)
    (24) —{↘}—(c)—[CaptureNode]→ (25)
    (25) —𝜀—[Field(c)]→ (27)
    (27) —{↗²}—𝜀→ (✓)
    (28) —(parent)→ (29)
    (29) —{↘}—(child)—[CaptureNode]→ (30)
    (30) —𝜀—[Field(c)]→ (31)
    (31) —{↗·¹}—𝜀→ (✓)
    (32) —(a)→ (33)
    (33) —{↘}—(b)→ (34)
    (34) —{↘}—(c)→ (35)
    (35) —{↘}—(d)→ (36)
    (36) —{↘}—(e)—[CaptureNode]→ (37)
    (37) —𝜀—[Field(e)]→ (41)
    (41) —{↗⁴}—𝜀→ (✓)
    (42) —(outer)→ (43)
    (43) —{↘.}—(first)—[CaptureNode]→ (44)
    (44) —𝜀—[Field(f)]→ (45)
    (45) —{→}—(middle)—[CaptureNode]→ (46)
    (46) —𝜀—[Field(m)]→ (47)
    (47) —{→·}—(last)—[CaptureNode]→ (48)
    (48) —𝜀—[Field(l)]→ (49)
    (49) —{↗·¹}—𝜀→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    NavUpMulti = { e: Node }
    NavUpAnchor = { c: Node }
    NavUp = { c: Node }
    NavStay = { r: Node }
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
    NavDownAnchor = { c: Node }
    NavDown = { c: Node }
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

    FlatScope = (00)
    BaseWithCapture = (08)
    RefOpaque = (10)
    RefCaptured = (12)
    TaggedAtRoot = (15)
    TaggedInline = (25)
    CardMult = (45)
    QisTwo = (54)
    NoQisOne = (63)
    MissingField = (67)
    SyntheticNames = (85)

    (00) —(a)→ (01)
    (01) —{↘}—(b)→ (02)
    (02) —{↘}—(c)→ (03)
    (03) —{↘}—(d)—[CaptureNode]→ (04)
    (04) —𝜀—[Field(val)]→ (07)
    (07) —{↗³}—𝜀→ (✓)
    (08) —(identifier)—[CaptureNode]→ (09)
    (09) —𝜀—[Field(name)]→ (✓)
    (10) —<BaseWithCapture>—𝜀→ (08), (11)
    (11) —𝜀—<BaseWithCapture>→ (✓)
    (12) —<BaseWithCapture>—𝜀→ (08), (13)
    (13) —𝜀—<BaseWithCapture>—[CaptureNode]→ (14)
    (14) —𝜀—[Field(result)]→ (✓)
    (15) —𝜀→ (18), (22)
    (16) —𝜀→ (✓)
    (18) —(a)—[StartVariant(A), CaptureNode]→ (20)
    (20) —𝜀—[Field(x), EndVariant]→ (16)
    (22) —(b)—[StartVariant(B), CaptureNode]→ (24)
    (24) —𝜀—[Field(y), EndVariant]→ (16)
    (25) —(wrapper)→ (26)
    (26) —{↘}—𝜀→ (29), (33)
    (29) —(a)—[StartVariant(A), CaptureNode]→ (31)
    (31) —𝜀—[Field(x), EndVariant]→ (36)
    (33) —(b)—[StartVariant(B), CaptureNode]→ (35)
    (35) —𝜀—[Field(y), EndVariant]→ (36)
    (36) —{↗¹}—𝜀→ (✓)
    (37) —(_)→ (39)
    (38) —{↘}—(item)—[CaptureNode]→ (42)
    (39) —𝜀—[StartArray]→ (38)
    (42) —𝜀—[PushElement]→ (38), (43)
    (43) —𝜀—[EndArray, Field(items)]→ (47)
    (45) —𝜀—[StartArray]→ (48)
    (46) —𝜀—[EndArray]→ (✓)
    (47) —{↗¹}—𝜀—[PushElement]→ (48)
    (48) —𝜀→ (37), (46)
    (49) —𝜀—[StartObject]→ (50)
    (50) —{→}—(a)—[CaptureNode]→ (51)
    (51) —𝜀—[Field(x)]→ (52)
    (52) —{→}—(b)—[CaptureNode]→ (58)
    (54) —𝜀—[StartArray]→ (59)
    (55) —𝜀—[EndArray]→ (✓)
    (58) —𝜀—[Field(y), EndObject, PushElement]→ (59)
    (59) —𝜀→ (49), (55)
    (61) —{→}—(a)—[CaptureNode]→ (65)
    (63) —𝜀—[StartArray]→ (66)
    (64) —𝜀—[EndArray]→ (✓)
    (65) —𝜀—[Field(x), PushElement]→ (66)
    (66) —𝜀→ (61), (64)
    (67) —𝜀→ (70), (80)
    (68) —𝜀→ (✓)
    (70) —(full)—[StartVariant(Full), StartObject]→ (71)
    (71) —{↘}—(a)—[CaptureNode]→ (72)
    (72) —𝜀—[Field(a)]→ (73)
    (73) —{→}—(b)—[CaptureNode]→ (74)
    (74) —𝜀—[Field(b)]→ (75)
    (75) —{→}—(c)—[CaptureNode]→ (76)
    (76) —𝜀—[Field(c)]→ (78)
    (78) —{↗¹}—𝜀—[EndObject, EndVariant]→ (68)
    (80) —(partial)—[StartVariant(Partial)]→ (81)
    (81) —{↘}—(a)—[CaptureNode]→ (82)
    (82) —𝜀—[Field(a)]→ (84)
    (84) —{↗¹}—𝜀—[EndVariant]→ (68)
    (85) —(foo)→ (89)
    (86) —{↘}—𝜀→ (87)
    (87) —{→}—(bar)—[CaptureNode, CaptureNode]→ (91)
    (89) —𝜀—[StartObject]→ (86)
    (91) —𝜀—[Field(bar), EndObject, Field(baz)]→ (92)
    (92) —{↗¹}—𝜀→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    RefOpaque = ()
    QisTwo = T12

    BaseWithCapture = { name: Node }
    TaggedInline = {
      x: Node?
      y: Node?
    }
    TaggedAtRoot = {
      A => Node
      B => Node
    }
    SyntheticNamesScope8 = { bar: Node }
    SyntheticNames = { baz: SyntheticNamesScope8 }
    RefCaptured = { result: BaseWithCapture }
    QisTwoScope11 = {
      x: Node
      y: Node
    }
    T12 = [QisTwoScope11]
    NoQisOne = { x: [Node] }
    MissingFieldScope15 = {
      a: Node
      b: Node
      c: Node
    }
    MissingField = {
      Full => MissingFieldScope15
      Partial => Node
    }
    FlatScope = { val: Node }
    CardMult = { items: [Node] }
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
    EffToString = (02)
    EffArray = (04)
    EffObject = (12)
    EffVariant = (20)
    EffClear = (33)

    (00) —(node)—[CaptureNode]→ (01)
    (01) —𝜀—[Field(name)]→ (✓)
    (02) —(node)—[CaptureNode, ToString]→ (03)
    (03) —𝜀—[Field(name)]→ (✓)
    (04) —(container)→ (06)
    (05) —{↘}—(item)—[CaptureNode]→ (08)
    (06) —𝜀—[StartArray]→ (09)
    (08) —𝜀—[PushElement]→ (09)
    (09) —𝜀→ (05), (10)
    (10) —𝜀—[EndArray, Field(items)]→ (11)
    (11) —{↗¹}—𝜀→ (✓)
    (12) —𝜀—[StartObject]→ (13)
    (13) —{→}—(a)—[CaptureNode, CaptureNode]→ (14)
    (14) —𝜀—[Field(x)]→ (15)
    (15) —{→}—(b)—[CaptureNode]→ (19)
    (19) —𝜀—[Field(y), EndObject, Field(obj)]→ (✓)
    (20) —𝜀—[StartObject]→ (23), (27)
    (23) —(a)—[StartVariant(A), CaptureNode, CaptureNode]→ (25)
    (25) —𝜀—[Field(x), EndVariant]→ (32)
    (27) —(b)—[StartVariant(B), CaptureNode, CaptureNode]→ (29)
    (29) —𝜀—[Field(y), EndVariant]→ (32)
    (32) —𝜀—[EndObject, Field(choice)]→ (✓)
    (33) —(container)→ (35)
    (34) —{↘}—(item)—[CaptureNode]→ (38)
    (35) —𝜀→ (34), (37)
    (37) —𝜀—[ClearCurrent]→ (38)
    (38) —𝜀—[Field(maybe)]→ (39)
    (39) —{↗¹}—𝜀→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    EffVariantScope3 = {
      A => Node
      B => Node
    }
    EffVariant = { choice: EffVariantScope3 }
    EffToString = { name: str }
    EffObjectScope6 = {
      x: Node
      y: Node
    }
    EffObject = { obj: EffObjectScope6 }
    EffClear = { maybe: Node? }
    EffCapture = { name: Node }
    EffArray = { items: [Node] }
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

    GreedyStar = (01)
    GreedyPlus = (07)
    Optional = (13)
    LazyStar = (18)
    LazyPlus = (24)
    QuantSeq = (34)
    NestedQuant = (48)

    (00) —(a)—[CaptureNode]→ (03)
    (01) —𝜀—[StartArray]→ (04)
    (03) —𝜀—[PushElement]→ (04)
    (04) —𝜀→ (00), (05)
    (05) —𝜀—[EndArray, Field(items)]→ (✓)
    (06) —(a)—[CaptureNode]→ (10)
    (07) —𝜀—[StartArray]→ (06)
    (10) —𝜀—[PushElement]→ (06), (11)
    (11) —𝜀—[EndArray, Field(items)]→ (✓)
    (12) —(a)—[CaptureNode]→ (16)
    (13) —𝜀→ (12), (15)
    (15) —𝜀—[ClearCurrent]→ (16)
    (16) —𝜀—[Field(maybe)]→ (✓)
    (17) —(a)—[CaptureNode]→ (20)
    (18) —𝜀—[StartArray]→ (21)
    (20) —𝜀—[PushElement]→ (21)
    (21) —𝜀→ (22), (17)
    (22) —𝜀—[EndArray, Field(items)]→ (✓)
    (23) —(a)—[CaptureNode]→ (27)
    (24) —𝜀—[StartArray]→ (23)
    (27) —𝜀—[PushElement]→ (28), (23)
    (28) —𝜀—[EndArray, Field(items)]→ (✓)
    (29) —𝜀—[StartObject]→ (30)
    (30) —{→}—(a)—[CaptureNode]→ (31)
    (31) —𝜀—[Field(x)]→ (32)
    (32) —{→}—(b)—[CaptureNode]→ (38)
    (34) —𝜀—[StartArray]→ (39)
    (35) —𝜀—[EndArray]→ (✓)
    (38) —𝜀—[Field(y), EndObject, PushElement]→ (39)
    (39) —𝜀→ (29), (35)
    (40) —(outer)—[CaptureNode]→ (42)
    (41) —{↘}—(inner)—[CaptureNode]→ (44)
    (42) —𝜀—[StartArray]→ (45)
    (44) —𝜀—[PushElement]→ (45)
    (45) —𝜀→ (41), (46)
    (46) —𝜀—[EndArray, Field(inners)]→ (51)
    (48) —𝜀—[StartArray]→ (40)
    (51) —{↗¹}—𝜀—[PushElement]→ (40), (52)
    (52) —𝜀—[EndArray, Field(outers)]→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    QuantSeq = T04

    QuantSeqScope3 = {
      x: Node
      y: Node
    }
    T04 = [QuantSeqScope3]
    Optional = { maybe: Node? }
    NestedQuant = {
      inners: [Node]
      outers: [Node]⁺
    }
    LazyStar = { items: [Node] }
    LazyPlus = { items: [Node]⁺ }
    GreedyStar = { items: [Node] }
    GreedyPlus = { items: [Node]⁺ }
    ");
}
