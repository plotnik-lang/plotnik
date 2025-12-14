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
    PlusQuant = (038)
    OptQuant = (045)
    QisNode = (057)
    QisSequence = (070)
    NoQis = (080)
    TaggedRoot = (084)
    TaggedCaptured = (096)
    TaggedMulti = (106)
    UntaggedSymmetric = (122)
    UntaggedAsymmetric = (130)
    UntaggedCaptured = (138)
    CapturedSeq = (142)
    UncapturedSeq = (147)
    NestedScopes = (157)
    Identifier = (161)
    RefSimple = (162)
    RefCaptured = (164)
    RefChain = (166)
    CardinalityJoin = (168)
    NestedQuant = (190)
    Complex = (197)
    WildcardCapture = (249)
    StringLiteral = (250)
    NoCaptures = (251)
    EmptyBranch = (252)

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
    (031) —(container)→ (033)
    (032) —{↘}—(item)—[CaptureNode]→ (035)
    (033) —𝜀—[StartArray]→ (036)
    (034) —𝜀—[EndArray]→ (037)
    (035) —𝜀—[PushElement]→ (036)
    (036) —𝜀→ (032), (034)
    (037) —{↗¹}—𝜀→ (✓)
    (038) —(container)→ (040)
    (039) —{↘}—(item)—[CaptureNode]→ (043)
    (040) —𝜀—[StartArray]→ (039)
    (041) —𝜀—[EndArray]→ (044)
    (043) —𝜀—[PushElement]→ (039), (041)
    (044) —{↗¹}—𝜀→ (✓)
    (045) —(container)→ (047)
    (046) —{↘}—(item)—[CaptureNode]→ (050)
    (047) —𝜀→ (046), (049)
    (049) —𝜀—[ClearCurrent]→ (050)
    (050) —{↗¹}—𝜀→ (✓)
    (051) —(function)—[StartObject]→ (052)
    (052) —{↘}—(identifier)@name—[CaptureNode]→ (053)
    (053) —𝜀—[Field(name)]→ (054)
    (054) —{→}—(block)@body—[CaptureNode]→ (055)
    (055) —𝜀—[Field(body)]→ (061)
    (057) —𝜀—[StartObject, StartArray]→ (062)
    (061) —{↗¹}—𝜀—[EndObject, PushElement]→ (062)
    (062) —𝜀→ (051), (064)
    (064) —𝜀—[EndArray, EndObject]→ (✓)
    (065) —𝜀—[StartObject]→ (066)
    (066) —{→}—(key)—[CaptureNode]→ (067)
    (067) —𝜀—[Field(key)]→ (068)
    (068) —{→}—(value)—[CaptureNode]→ (074)
    (070) —𝜀—[StartObject, StartArray]→ (075)
    (074) —𝜀—[Field(value), EndObject, PushElement]→ (075)
    (075) —𝜀→ (065), (077)
    (077) —𝜀—[EndArray, EndObject]→ (✓)
    (079) —{→}—(item)—[CaptureNode]→ (082)
    (080) —𝜀—[StartArray]→ (083)
    (081) —𝜀—[EndArray]→ (✓)
    (082) —𝜀—[PushElement]→ (083)
    (083) —𝜀→ (079), (081)
    (084) —𝜀—[StartObject]→ (087), (091)
    (087) —(success)—[StartVariant(Ok), CaptureNode]→ (089)
    (089) —𝜀—[Field(val), EndVariant]→ (095)
    (091) —(error)—[StartVariant(Err), CaptureNode, ToString]→ (093)
    (093) —𝜀—[Field(msg), EndVariant]→ (095)
    (095) —𝜀—[EndObject]→ (✓)
    (096) —(wrapper)→ (097)
    (097) —{↘}—𝜀→ (100), (103)
    (100) —(left_node)—[StartVariant(Left), CaptureNode, CaptureNode]→ (101)
    (101) —𝜀—[EndVariant]→ (105)
    (103) —(right_node)—[StartVariant(Right), CaptureNode, CaptureNode]→ (104)
    (104) —𝜀—[EndVariant]→ (105)
    (105) —{↗¹}—𝜀→ (✓)
    (106) —𝜀—[StartObject]→ (109), (113)
    (109) —(node)—[StartVariant(Simple), CaptureNode]→ (111)
    (111) —𝜀—[Field(val), EndVariant]→ (121)
    (113) —(pair)—[StartVariant(Complex), StartObject]→ (114)
    (114) —{↘}—(key)—[CaptureNode]→ (115)
    (115) —𝜀—[Field(k)]→ (116)
    (116) —{→}—(value)—[CaptureNode]→ (117)
    (117) —𝜀—[Field(v)]→ (119)
    (119) —{↗¹}—𝜀—[EndObject, EndVariant]→ (121)
    (121) —𝜀—[EndObject]→ (✓)
    (122) —𝜀—[StartObject]→ (124), (126)
    (124) —(a)—[CaptureNode]→ (125)
    (125) —𝜀—[Field(val)]→ (129)
    (126) —(b)—[CaptureNode]→ (127)
    (127) —𝜀—[Field(val)]→ (129)
    (129) —𝜀—[EndObject]→ (✓)
    (130) —𝜀—[StartObject]→ (132), (134)
    (132) —(a)—[CaptureNode]→ (133)
    (133) —𝜀—[Field(x)]→ (137)
    (134) —(b)—[CaptureNode]→ (135)
    (135) —𝜀—[Field(y)]→ (137)
    (137) —𝜀—[EndObject]→ (✓)
    (138) —𝜀→ (140), (141)
    (139) —𝜀→ (✓)
    (140) —(a)—[CaptureNode, CaptureNode]→ (139)
    (141) —(b)—[CaptureNode, CaptureNode]→ (139)
    (142) —(outer)→ (143)
    (143) —{↘}—𝜀→ (144)
    (144) —{→}—(inner)—[CaptureNode, CaptureNode]→ (145)
    (145) —{→}—(inner2)—[CaptureNode]→ (146)
    (146) —{↗¹}—𝜀→ (✓)
    (147) —(outer)—[StartObject]→ (148)
    (148) —{↘}—𝜀→ (149)
    (149) —{→}—(inner)—[CaptureNode]→ (150)
    (150) —𝜀—[Field(x)]→ (151)
    (151) —{→}—(inner2)—[CaptureNode]→ (152)
    (152) —𝜀—[Field(y)]→ (155)
    (155) —{↗¹}—𝜀—[EndObject]→ (✓)
    (157) —{→}—𝜀→ (158)
    (158) —{→}—(a)—[CaptureNode, CaptureNode, CaptureNode]→ (159)
    (159) —{→}—𝜀→ (160)
    (160) —{→}—(b)—[CaptureNode, CaptureNode]→ (✓)
    (161) —(identifier)—[CaptureNode]→ (✓)
    (162) —<Identifier>—𝜀→ (161), (163)
    (163) —𝜀—<Identifier>→ (✓)
    (164) —<Identifier>—𝜀→ (161), (165)
    (165) —𝜀—<Identifier>—[CaptureNode]→ (✓)
    (166) —<RefSimple>—𝜀→ (162), (167)
    (167) —𝜀—<RefSimple>→ (✓)
    (168) —𝜀—[StartObject]→ (170), (172)
    (170) —(single)—[CaptureNode]→ (171)
    (171) —𝜀—[Field(item)]→ (181)
    (172) —(multi)→ (174)
    (173) —{↘}—(x)—[CaptureNode]→ (177)
    (174) —𝜀—[StartArray]→ (173)
    (177) —𝜀—[PushElement]→ (173), (178)
    (178) —𝜀—[EndArray, Field(item)]→ (181)
    (181) —{↗¹}—𝜀—[EndObject]→ (✓)
    (182) —(_)—[CaptureNode]→ (184)
    (183) —{↘}—(item)—[CaptureNode]→ (186)
    (184) —𝜀—[StartArray]→ (187)
    (186) —𝜀—[PushElement]→ (187)
    (187) —𝜀→ (183), (188)
    (188) —𝜀—[EndArray, Field(inner)]→ (193)
    (190) —𝜀—[StartObject, StartArray]→ (182)
    (193) —{↗¹}—𝜀—[PushElement]→ (182), (196)
    (196) —𝜀—[EndArray, Field(outer), EndObject]→ (✓)
    (197) —(module)—[StartObject]→ (198)
    (198) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (201)
    (200) —{→·}—(import)—[CaptureNode]→ (203)
    (201) —𝜀—[Field(mod_name), StartArray]→ (204)
    (203) —𝜀—[PushElement]→ (204)
    (204) —𝜀→ (200), (205)
    (205) —𝜀—[EndArray, Field(imports)]→ (206)
    (206) —{→}—(block)@body→ (236)
    (207) —{↘}—𝜀→ (208)
    (208) —{→}—𝜀→ (211), (229)
    (211) —(function)—[StartVariant(Func), StartObject, CaptureNode]→ (212)
    (212) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (213)
    (213) —𝜀—[Field(fn_name)]→ (214)
    (214) —{→}—(parameters)@params→ (218)
    (215) —{↘}—𝜀→ (216)
    (216) —{→}—(param)—[CaptureNode, CaptureNode]→ (220)
    (218) —𝜀—[StartArray]→ (221)
    (220) —𝜀—[Field(p), PushElement]→ (221)
    (221) —𝜀→ (215), (222)
    (222) —𝜀—[EndArray, Field(params)]→ (223)
    (223) —{↗¹}—𝜀→ (224)
    (224) —{→}—(block)@body—[CaptureNode]→ (225)
    (225) —𝜀—[Field(fn_body)]→ (227)
    (227) —{↗¹}—𝜀—[EndObject, EndVariant]→ (240)
    (229) —(class)—[StartVariant(Class), StartObject, CaptureNode]→ (230)
    (230) —{↘}—(identifier)@name—[CaptureNode, ToString]→ (231)
    (231) —𝜀—[Field(cls_name)]→ (232)
    (232) —{→}—(class_body)@body—[CaptureNode]→ (233)
    (233) —𝜀—[Field(cls_body)]→ (235)
    (235) —{↗¹}—𝜀—[EndObject, EndVariant]→ (240)
    (236) —𝜀—[StartObject, StartArray]→ (241)
    (238) —𝜀—[StartObject]→ (207)
    (240) —𝜀—[EndObject, PushElement]→ (241)
    (241) —𝜀→ (238), (244)
    (244) —𝜀—[EndArray, EndObject, Field(items)]→ (245)
    (245) —{↗¹}—𝜀→ (248)
    (248) —{↗·¹}—𝜀—[EndObject]→ (✓)
    (249) —(🞵)—[CaptureNode]→ (✓)
    (250) —"+"—[CaptureNode]→ (✓)
    (251) —(identifier)→ (✓)
    (252) —𝜀→ (255), (258)
    (253) —𝜀→ (✓)
    (255) —(value)—[StartVariant(Some), CaptureNode]→ (256)
    (256) —𝜀—[EndVariant]→ (253)
    (258) —(none_marker)—[StartVariant(None)]→ (259)
    (259) —𝜀—[EndVariant]→ (253)

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
    DeepNest = Node
    CardinalityJoin = [Node]⁺
    CapturedSeq = CapturedSeqScope41
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
    ComplexScope30 = {
      fn_name: str?
      p: [Node]
      params: [Node]
      fn_body: Node?
      cls_name: str?
      cls_body: Node?
    }
    T37 = [ComplexScope30]
    Complex = {
      mod_name: str
      imports: [Node]
      items: T37
    }
    CapturedSeqScope41 = {
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

    FlatScope = (00)
    BaseWithCapture = (07)
    RefOpaque = (08)
    RefCaptured = (10)
    TaggedAtRoot = (12)
    TaggedInline = (24)
    CardMult = (45)
    QisTwo = (54)
    NoQisOne = (64)
    MissingField = (68)
    SyntheticNames = (88)

    (00) —(a)→ (01)
    (01) —{↘}—(b)→ (02)
    (02) —{↘}—(c)→ (03)
    (03) —{↘}—(d)—[CaptureNode]→ (06)
    (06) —{↗³}—𝜀→ (✓)
    (07) —(identifier)—[CaptureNode]→ (✓)
    (08) —<BaseWithCapture>—𝜀→ (07), (09)
    (09) —𝜀—<BaseWithCapture>→ (✓)
    (10) —<BaseWithCapture>—𝜀→ (07), (11)
    (11) —𝜀—<BaseWithCapture>—[CaptureNode]→ (✓)
    (12) —𝜀—[StartObject]→ (15), (19)
    (15) —(a)—[StartVariant(A), CaptureNode]→ (17)
    (17) —𝜀—[Field(x), EndVariant]→ (23)
    (19) —(b)—[StartVariant(B), CaptureNode]→ (21)
    (21) —𝜀—[Field(y), EndVariant]→ (23)
    (23) —𝜀—[EndObject]→ (✓)
    (24) —(wrapper)—[StartObject]→ (25)
    (25) —{↘}—𝜀→ (28), (32)
    (28) —(a)—[StartVariant(A), CaptureNode]→ (30)
    (30) —𝜀—[Field(x), EndVariant]→ (37)
    (32) —(b)—[StartVariant(B), CaptureNode]→ (34)
    (34) —𝜀—[Field(y), EndVariant]→ (37)
    (37) —{↗¹}—𝜀—[EndObject]→ (✓)
    (38) —(_)→ (40)
    (39) —{↘}—(item)—[CaptureNode]→ (43)
    (40) —𝜀—[StartArray]→ (39)
    (41) —𝜀—[EndArray]→ (47)
    (43) —𝜀—[PushElement]→ (39), (41)
    (45) —𝜀—[StartArray]→ (48)
    (46) —𝜀—[EndArray]→ (✓)
    (47) —{↗¹}—𝜀—[PushElement]→ (48)
    (48) —𝜀→ (38), (46)
    (49) —𝜀—[StartObject]→ (50)
    (50) —{→}—(a)—[CaptureNode]→ (51)
    (51) —𝜀—[Field(x)]→ (52)
    (52) —{→}—(b)—[CaptureNode]→ (58)
    (54) —𝜀—[StartObject, StartArray]→ (59)
    (58) —𝜀—[Field(y), EndObject, PushElement]→ (59)
    (59) —𝜀→ (49), (61)
    (61) —𝜀—[EndArray, EndObject]→ (✓)
    (63) —{→}—(a)—[CaptureNode]→ (66)
    (64) —𝜀—[StartArray]→ (67)
    (65) —𝜀—[EndArray]→ (✓)
    (66) —𝜀—[PushElement]→ (67)
    (67) —𝜀→ (63), (65)
    (68) —𝜀—[StartObject]→ (71), (81)
    (71) —(full)—[StartVariant(Full), StartObject]→ (72)
    (72) —{↘}—(a)—[CaptureNode]→ (73)
    (73) —𝜀—[Field(a)]→ (74)
    (74) —{→}—(b)—[CaptureNode]→ (75)
    (75) —𝜀—[Field(b)]→ (76)
    (76) —{→}—(c)—[CaptureNode]→ (77)
    (77) —𝜀—[Field(c)]→ (79)
    (79) —{↗¹}—𝜀—[EndObject, EndVariant]→ (87)
    (81) —(partial)—[StartVariant(Partial)]→ (82)
    (82) —{↘}—(a)—[CaptureNode]→ (83)
    (83) —𝜀—[Field(a)]→ (85)
    (85) —{↗¹}—𝜀—[EndVariant]→ (87)
    (87) —𝜀—[EndObject]→ (✓)
    (88) —(foo)→ (89)
    (89) —{↘}—𝜀→ (90)
    (90) —{→}—(bar)—[CaptureNode, CaptureNode]→ (91)
    (91) —{↗¹}—𝜀→ (✓)

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
    EffObject = (10)
    EffVariant = (12)
    EffClear = (20)

    (00) —(node)—[CaptureNode]→ (✓)
    (01) —(node)—[CaptureNode, ToString]→ (✓)
    (02) —(container)→ (04)
    (03) —{↘}—(item)—[CaptureNode]→ (06)
    (04) —𝜀—[StartArray]→ (07)
    (05) —𝜀—[EndArray]→ (08)
    (06) —𝜀—[PushElement]→ (07)
    (07) —𝜀→ (03), (05)
    (08) —{↗¹}—𝜀→ (✓)
    (10) —{→}—(a)—[CaptureNode, CaptureNode]→ (11)
    (11) —{→}—(b)—[CaptureNode]→ (✓)
    (12) —𝜀→ (15), (18)
    (13) —𝜀→ (✓)
    (15) —(a)—[StartVariant(A), CaptureNode, CaptureNode]→ (16)
    (16) —𝜀—[EndVariant]→ (13)
    (18) —(b)—[StartVariant(B), CaptureNode, CaptureNode]→ (19)
    (19) —𝜀—[EndVariant]→ (13)
    (20) —(container)→ (22)
    (21) —{↘}—(item)—[CaptureNode]→ (25)
    (22) —𝜀→ (21), (24)
    (24) —𝜀—[ClearCurrent]→ (25)
    (25) —{↗¹}—𝜀→ (✓)

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

    GreedyStar = (01)
    GreedyPlus = (06)
    Optional = (11)
    LazyStar = (15)
    LazyPlus = (20)
    QuantSeq = (29)
    NestedQuant = (45)

    (00) —(a)—[CaptureNode]→ (03)
    (01) —𝜀—[StartArray]→ (04)
    (02) —𝜀—[EndArray]→ (✓)
    (03) —𝜀—[PushElement]→ (04)
    (04) —𝜀→ (00), (02)
    (05) —(a)—[CaptureNode]→ (09)
    (06) —𝜀—[StartArray]→ (05)
    (07) —𝜀—[EndArray]→ (✓)
    (09) —𝜀—[PushElement]→ (05), (07)
    (10) —(a)—[CaptureNode]→ (12)
    (11) —𝜀→ (10), (13)
    (12) —𝜀→ (✓)
    (13) —𝜀—[ClearCurrent]→ (12)
    (14) —(a)—[CaptureNode]→ (17)
    (15) —𝜀—[StartArray]→ (18)
    (16) —𝜀—[EndArray]→ (✓)
    (17) —𝜀—[PushElement]→ (18)
    (18) —𝜀→ (16), (14)
    (19) —(a)—[CaptureNode]→ (23)
    (20) —𝜀—[StartArray]→ (19)
    (21) —𝜀—[EndArray]→ (✓)
    (23) —𝜀—[PushElement]→ (21), (19)
    (24) —𝜀—[StartObject]→ (25)
    (25) —{→}—(a)—[CaptureNode]→ (26)
    (26) —𝜀—[Field(x)]→ (27)
    (27) —{→}—(b)—[CaptureNode]→ (33)
    (29) —𝜀—[StartObject, StartArray]→ (34)
    (33) —𝜀—[Field(y), EndObject, PushElement]→ (34)
    (34) —𝜀→ (24), (36)
    (36) —𝜀—[EndArray, EndObject]→ (✓)
    (37) —(outer)—[CaptureNode]→ (39)
    (38) —{↘}—(inner)—[CaptureNode]→ (41)
    (39) —𝜀—[StartArray]→ (42)
    (41) —𝜀—[PushElement]→ (42)
    (42) —𝜀→ (38), (43)
    (43) —𝜀—[EndArray, Field(inners)]→ (48)
    (45) —𝜀—[StartObject, StartArray]→ (37)
    (48) —{↗¹}—𝜀—[PushElement]→ (37), (51)
    (51) —𝜀—[EndArray, Field(outers), EndObject]→ (✓)

    ═══════════════════════════════════════════════════════════════════════════════
                                  TYPE INFERENCE
    ═══════════════════════════════════════════════════════════════════════════════

    QuantSeq = T04
    Optional = Node?
    LazyStar = [Node]
    LazyPlus = [Node]⁺
    GreedyStar = [Node]
    GreedyPlus = [Node]⁺

    QuantSeqScope3 = {
      x: Node
      y: Node
    }
    T04 = [QuantSeqScope3]
    NestedQuant = {
      inners: [Node]
      outers: [Node]⁺
    }
    ");
}
