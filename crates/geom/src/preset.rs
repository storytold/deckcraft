//! Preset shape geometry.
//!
//! Each preset is identified by its Office Open XML name (`rect`, `roundRect`, `rightArrow`…) so
//! files round-trip, but every outline here is our own construction in Rust: we re-derive the shapes
//! from what they look like and from the meaning of their adjust values, not from any reference
//! geometry table. Adjust values use the file convention: ratios in 1/100 000 of the shorter side (or
//! of the width/height), angles in 1/60 000 degree.

use kurbo::{Arc, BezPath, Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

mod extra;

/// How a sub-path is filled. `Lighten`/`Darken` variants shade the shape's fill (3-D looking faces).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillMode {
    Norm,
    None,
    Lighten,
    LightenLess,
    Darken,
    DarkenLess,
}

#[derive(Clone, Debug)]
pub struct SubPath {
    pub path: BezPath,
    pub fill: FillMode,
    pub stroke: bool,
    /// Even-odd fill (holes in donuts and frames).
    pub even_odd: bool,
}

/// A draggable adjust handle (the yellow dot). `pos = origin + dir * value`; dragging projects
/// the pointer onto `dir`. An `angle` handle instead measures the pointer's angle about `origin`
/// (1/60 000 degree, y down, 0° at +x).
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    pub index: usize,
    pub pos: Point,
    pub origin: Point,
    pub dir: Vec2,
    pub min: f64,
    pub max: f64,
    pub angle: bool,
}

impl Handle {
    fn new(index: usize, origin: Point, dir: Vec2, value: f64, min: f64, max: f64) -> Self {
        Handle { index, pos: origin + dir * value, origin, dir, min, max, angle: false }
    }
    /// An angle handle on the `rx`×`ry` ellipse around `center` at visual angle `value`.
    fn angle(index: usize, center: Point, rx: f64, ry: f64, value: f64) -> Self {
        let t = vis(value / 60000.0, rx, ry).to_radians();
        let pos = Point::new(center.x + rx * t.cos(), center.y + ry * t.sin());
        Handle { index, pos, origin: center, dir: Vec2::ZERO, min: 0.0, max: 21600000.0, angle: true }
    }
    /// The adjust value for a pointer at `p` (shape-local space).
    pub fn value_at(&self, p: Point) -> f64 {
        if self.angle {
            let d = p - self.origin;
            if !(d.x.is_finite() && d.y.is_finite()) || d.hypot() < 1e-9 {
                return self.min;
            }
            let v = (d.y.atan2(d.x).to_degrees().rem_euclid(360.0) * 60000.0).round();
            return if v >= self.max { self.min } else { v };
        }
        let l2 = self.dir.hypot2();
        if l2 < 1e-18 {
            return self.min;
        }
        let v = (p - self.origin).dot(self.dir) / l2;
        if !v.is_finite() {
            return self.min;
        }
        v.clamp(self.min.min(self.max), self.max.max(self.min)).round()
    }
}

#[derive(Clone, Debug)]
pub struct Geometry {
    pub paths: Vec<SubPath>,
    /// Where text goes, in shape-local space.
    pub text_rect: Rect,
    pub handles: Vec<Handle>,
    /// Connector glue points, shape-local.
    pub sites: Vec<Point>,
}

impl Geometry {
    /// The union outline used for hit testing and selection (all filled or stroked sub-paths).
    pub fn outline(&self) -> BezPath {
        let mut p = BezPath::new();
        for s in &self.paths {
            p.extend(s.path.iter());
        }
        p
    }
    /// Whether nothing is filled (lines, connectors, arcs, brackets).
    pub fn is_open(&self) -> bool {
        self.paths.iter().all(|s| s.fill == FillMode::None)
    }
}

/// Gallery category, as in the Shapes menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Lines,
    Rectangles,
    BasicShapes,
    BlockArrows,
    EquationShapes,
    Flowchart,
    StarsAndBanners,
    Callouts,
    ActionButtons,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Lines,
        Category::Rectangles,
        Category::BasicShapes,
        Category::BlockArrows,
        Category::EquationShapes,
        Category::Flowchart,
        Category::StarsAndBanners,
        Category::Callouts,
        Category::ActionButtons,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Category::Lines => "Lines",
            Category::Rectangles => "Rectangles",
            Category::BasicShapes => "Basic Shapes",
            Category::BlockArrows => "Block Arrows",
            Category::EquationShapes => "Equation Shapes",
            Category::Flowchart => "Flowchart",
            Category::StarsAndBanners => "Stars and Banners",
            Category::Callouts => "Callouts",
            Category::ActionButtons => "Action Buttons",
        }
    }
}

pub struct PresetInfo {
    pub name: &'static str,
    pub label: &'static str,
    pub category: Category,
    /// Default adjust values.
    pub defaults: &'static [f64],
}

macro_rules! p {
    ($n:literal, $l:literal, $c:ident, [$($d:expr),*]) => {
        PresetInfo { name: $n, label: $l, category: Category::$c, defaults: &[$($d as f64),*] }
    };
}

/// Every preset we generate, in gallery order.
pub static CATALOG: &[PresetInfo] = &[
    p!("line", "Line", Lines, []),
    p!("straightConnector1", "Arrow", Lines, []),
    p!("bentConnector3", "Connector: Elbow", Lines, [50000]),
    p!("curvedConnector3", "Connector: Curved", Lines, [50000]),
    p!("bentConnector2", "Connector: Elbow (one bend)", Lines, []),
    p!("lineInv", "Line: Rising", Lines, []),
    p!("rect", "Rectangle", Rectangles, []),
    p!("roundRect", "Rectangle: Rounded Corners", Rectangles, [16667]),
    p!("snip1Rect", "Rectangle: Single Corner Snipped", Rectangles, [16667]),
    p!("snip2SameRect", "Rectangle: Top Corners Snipped", Rectangles, [16667, 0]),
    p!("snip2DiagRect", "Rectangle: Diagonal Corners Snipped", Rectangles, [0, 16667]),
    p!("snipRoundRect", "Rectangle: Top Corners One Rounded and One Snipped", Rectangles, [16667, 16667]),
    p!("round1Rect", "Rectangle: Single Corner Rounded", Rectangles, [16667]),
    p!("round2SameRect", "Rectangle: Top Corners Rounded", Rectangles, [16667, 0]),
    p!("round2DiagRect", "Rectangle: Diagonal Corners Rounded", Rectangles, [16667, 0]),
    p!("textBox", "Text Box", BasicShapes, []),
    p!("ellipse", "Oval", BasicShapes, []),
    p!("triangle", "Isosceles Triangle", BasicShapes, [50000]),
    p!("rtTriangle", "Right Triangle", BasicShapes, []),
    p!("parallelogram", "Parallelogram", BasicShapes, [25000]),
    p!("trapezoid", "Trapezoid", BasicShapes, [25000]),
    p!("nonIsoscelesTrapezoid", "Trapezoid: Uneven Sides", BasicShapes, [25000, 25000]),
    p!("diamond", "Diamond", BasicShapes, []),
    p!("pentagon", "Regular Pentagon", BasicShapes, []),
    p!("hexagon", "Hexagon", BasicShapes, [25000]),
    p!("heptagon", "Heptagon", BasicShapes, []),
    p!("octagon", "Octagon", BasicShapes, [29289]),
    p!("decagon", "Decagon", BasicShapes, []),
    p!("dodecagon", "Dodecagon", BasicShapes, []),
    p!("pie", "Partial Circle", BasicShapes, [0, 16200000]),
    p!("pieWedge", "Quarter Wedge", BasicShapes, []),
    p!("chord", "Chord", BasicShapes, [2700000, 16200000]),
    p!("teardrop", "Teardrop", BasicShapes, [100000]),
    p!("frame", "Frame", BasicShapes, [12500]),
    p!("cornerTabs", "Corner Tabs: Triangles", BasicShapes, []),
    p!("squareTabs", "Corner Tabs: Squares", BasicShapes, []),
    p!("plaqueTabs", "Corner Tabs: Quarter Circles", BasicShapes, []),
    p!("halfFrame", "Half Frame", BasicShapes, [33333, 33333]),
    p!("corner", "L-Shape", BasicShapes, [50000, 50000]),
    p!("diagStripe", "Diagonal Stripe", BasicShapes, [50000]),
    p!("plus", "Cross", BasicShapes, [25000]),
    p!("plaque", "Plaque", BasicShapes, [16667]),
    p!("can", "Cylinder", BasicShapes, [25000]),
    p!("cube", "Cube", BasicShapes, [25000]),
    p!("bevel", "Bevel", BasicShapes, [12500]),
    p!("donut", "Donut", BasicShapes, [25000]),
    p!("noSmoking", "\"Not Allowed\" Symbol", BasicShapes, [18750]),
    p!("blockArc", "Block Arc", BasicShapes, [10800000, 0, 25000]),
    p!("foldedCorner", "Folded Corner", BasicShapes, [16667]),
    p!("smileyFace", "Smiley Face", BasicShapes, [4653]),
    p!("heart", "Heart", BasicShapes, []),
    p!("lightningBolt", "Lightning Bolt", BasicShapes, []),
    p!("sun", "Sun", BasicShapes, [25000]),
    p!("moon", "Moon", BasicShapes, [50000]),
    p!("cloud", "Cloud", BasicShapes, []),
    p!("gear6", "Gear: 6 Teeth", BasicShapes, [15000, 3526]),
    p!("gear9", "Gear: 9 Teeth", BasicShapes, [10000, 1763]),
    p!("funnel", "Funnel", BasicShapes, []),
    p!("arc", "Arc", BasicShapes, [16200000, 0]),
    p!("bracketPair", "Double Bracket", BasicShapes, [16667]),
    p!("bracePair", "Double Brace", BasicShapes, [8333]),
    p!("leftBracket", "Left Bracket", BasicShapes, [8333]),
    p!("rightBracket", "Right Bracket", BasicShapes, [8333]),
    p!("leftBrace", "Left Brace", BasicShapes, [8333, 50000]),
    p!("rightBrace", "Right Brace", BasicShapes, [8333, 50000]),
    p!("chartX", "Chart Marker: Cross Lines", BasicShapes, []),
    p!("chartStar", "Chart Marker: Star Lines", BasicShapes, []),
    p!("chartPlus", "Chart Marker: Plus Lines", BasicShapes, []),
    p!("rightArrow", "Arrow: Right", BlockArrows, [50000, 50000]),
    p!("leftArrow", "Arrow: Left", BlockArrows, [50000, 50000]),
    p!("upArrow", "Arrow: Up", BlockArrows, [50000, 50000]),
    p!("downArrow", "Arrow: Down", BlockArrows, [50000, 50000]),
    p!("leftRightArrow", "Arrow: Left-Right", BlockArrows, [50000, 50000]),
    p!("upDownArrow", "Arrow: Up-Down", BlockArrows, [50000, 50000]),
    p!("quadArrow", "Arrow: Quad", BlockArrows, [22500, 22500, 22500]),
    p!("leftRightUpArrow", "Arrow: Left-Right-Up", BlockArrows, [25000, 25000, 25000]),
    p!("bentArrow", "Arrow: Bent", BlockArrows, [25000, 25000, 25000, 43750]),
    p!("uturnArrow", "Arrow: U-Turn", BlockArrows, [25000, 25000, 25000, 43750, 75000]),
    p!("curvedRightArrow", "Arrow: Curved Right", BlockArrows, [25000, 50000, 25000]),
    p!("curvedLeftArrow", "Arrow: Curved Left", BlockArrows, [25000, 50000, 25000]),
    p!("curvedUpArrow", "Arrow: Curved Up", BlockArrows, [25000, 50000, 25000]),
    p!("curvedDownArrow", "Arrow: Curved Down", BlockArrows, [25000, 50000, 25000]),
    p!("leftUpArrow", "Arrow: Left-Up", BlockArrows, [25000, 25000, 25000]),
    p!("bentUpArrow", "Arrow: Bent-Up", BlockArrows, [25000, 25000, 25000]),
    p!("stripedRightArrow", "Arrow: Striped Right", BlockArrows, [50000, 50000]),
    p!("notchedRightArrow", "Arrow: Notched Right", BlockArrows, [50000, 50000]),
    p!("homePlate", "Arrow: Pentagon", BlockArrows, [50000]),
    p!("chevron", "Arrow: Chevron", BlockArrows, [50000]),
    p!("circularArrow", "Arrow: Circular", BlockArrows, [12500, 1142319, 20457681, 10800000, 12500]),
    p!("leftCircularArrow", "Arrow: Circular Counter-clockwise", BlockArrows, [12500, -1142319, 1142319, 10800000, 12500]),
    p!("leftRightCircularArrow", "Arrow: Circular Two-headed", BlockArrows, [12500, 1142319, 20457681, 11942319, 12500]),
    p!("swooshArrow", "Arrow: Swoosh", BlockArrows, [25000, 16667]),
    p!("rightArrowCallout", "Callout: Right Arrow", BlockArrows, [25000, 25000, 25000, 64977]),
    p!("downArrowCallout", "Callout: Down Arrow", BlockArrows, [25000, 25000, 25000, 64977]),
    p!("leftArrowCallout", "Callout: Left Arrow", BlockArrows, [25000, 25000, 25000, 64977]),
    p!("upArrowCallout", "Callout: Up Arrow", BlockArrows, [25000, 25000, 25000, 64977]),
    p!("leftRightArrowCallout", "Callout: Left-Right Arrow", BlockArrows, [25000, 25000, 25000, 48123]),
    p!("upDownArrowCallout", "Callout: Up-Down Arrow", BlockArrows, [25000, 25000, 25000, 48123]),
    p!("quadArrowCallout", "Callout: Quad Arrow", BlockArrows, [18515, 18515, 18515, 48123]),
    p!("mathPlus", "Plus Sign", EquationShapes, [23520]),
    p!("mathMinus", "Minus Sign", EquationShapes, [23520]),
    p!("mathMultiply", "Multiplication Sign", EquationShapes, [23520]),
    p!("mathDivide", "Division Sign", EquationShapes, [23520, 5880, 11760]),
    p!("mathEqual", "Equals", EquationShapes, [23520, 11760]),
    p!("mathNotEqual", "Not Equal", EquationShapes, [23520, 6600000, 11760]),
    p!("flowChartProcess", "Flowchart: Process", Flowchart, []),
    p!("flowChartAlternateProcess", "Flowchart: Alternate Process", Flowchart, []),
    p!("flowChartDecision", "Flowchart: Decision", Flowchart, []),
    p!("flowChartInputOutput", "Flowchart: Data", Flowchart, []),
    p!("flowChartPredefinedProcess", "Flowchart: Predefined Process", Flowchart, []),
    p!("flowChartInternalStorage", "Flowchart: Internal Storage", Flowchart, []),
    p!("flowChartDocument", "Flowchart: Document", Flowchart, []),
    p!("flowChartMultidocument", "Flowchart: Multidocument", Flowchart, []),
    p!("flowChartTerminator", "Flowchart: Terminator", Flowchart, []),
    p!("flowChartPreparation", "Flowchart: Preparation", Flowchart, []),
    p!("flowChartManualInput", "Flowchart: Manual Input", Flowchart, []),
    p!("flowChartManualOperation", "Flowchart: Manual Operation", Flowchart, []),
    p!("flowChartConnector", "Flowchart: Connector", Flowchart, []),
    p!("flowChartOffpageConnector", "Flowchart: Off-page Connector", Flowchart, []),
    p!("flowChartPunchedCard", "Flowchart: Card", Flowchart, []),
    p!("flowChartPunchedTape", "Flowchart: Punched Tape", Flowchart, []),
    p!("flowChartSummingJunction", "Flowchart: Summing Junction", Flowchart, []),
    p!("flowChartOr", "Flowchart: Or", Flowchart, []),
    p!("flowChartCollate", "Flowchart: Collate", Flowchart, []),
    p!("flowChartSort", "Flowchart: Sort", Flowchart, []),
    p!("flowChartExtract", "Flowchart: Extract", Flowchart, []),
    p!("flowChartMerge", "Flowchart: Merge", Flowchart, []),
    p!("flowChartOnlineStorage", "Flowchart: Stored Data", Flowchart, []),
    p!("flowChartDelay", "Flowchart: Delay", Flowchart, []),
    p!("flowChartMagneticTape", "Flowchart: Sequential Access Storage", Flowchart, []),
    p!("flowChartMagneticDisk", "Flowchart: Magnetic Disk", Flowchart, []),
    p!("flowChartMagneticDrum", "Flowchart: Direct Access Storage", Flowchart, []),
    p!("flowChartDisplay", "Flowchart: Display", Flowchart, []),
    p!("flowChartOfflineStorage", "Flowchart: Offline Storage", Flowchart, []),
    p!("irregularSeal1", "Explosion: 8 Points", StarsAndBanners, []),
    p!("irregularSeal2", "Explosion: 14 Points", StarsAndBanners, []),
    p!("star4", "Star: 4 Points", StarsAndBanners, [12500]),
    p!("star5", "Star: 5 Points", StarsAndBanners, [19098]),
    p!("star6", "Star: 6 Points", StarsAndBanners, [28868]),
    p!("star7", "Star: 7 Points", StarsAndBanners, [34601]),
    p!("star8", "Star: 8 Points", StarsAndBanners, [37500]),
    p!("star10", "Star: 10 Points", StarsAndBanners, [42533]),
    p!("star12", "Star: 12 Points", StarsAndBanners, [37500]),
    p!("star16", "Star: 16 Points", StarsAndBanners, [37500]),
    p!("star24", "Star: 24 Points", StarsAndBanners, [37500]),
    p!("star32", "Star: 32 Points", StarsAndBanners, [37500]),
    p!("ribbon2", "Ribbon: Tilted Up", StarsAndBanners, [16667, 50000]),
    p!("ribbon", "Ribbon: Tilted Down", StarsAndBanners, [16667, 50000]),
    p!("ellipseRibbon2", "Ribbon: Curved and Tilted Up", StarsAndBanners, [25000, 50000, 12500]),
    p!("ellipseRibbon", "Ribbon: Curved and Tilted Down", StarsAndBanners, [25000, 50000, 12500]),
    p!("leftRightRibbon", "Ribbon: Left-Right Fold", StarsAndBanners, [50000, 50000, 16667]),
    p!("verticalScroll", "Scroll: Vertical", StarsAndBanners, [12500]),
    p!("horizontalScroll", "Scroll: Horizontal", StarsAndBanners, [12500]),
    p!("wave", "Wave", StarsAndBanners, [12500, 0]),
    p!("doubleWave", "Double Wave", StarsAndBanners, [6250, 0]),
    p!("wedgeRectCallout", "Speech Bubble: Rectangle", Callouts, [-20833, 62500]),
    p!("wedgeRoundRectCallout", "Speech Bubble: Rectangle with Corners Rounded", Callouts, [-20833, 62500, 16667]),
    p!("wedgeEllipseCallout", "Speech Bubble: Oval", Callouts, [-20833, 62500]),
    p!("cloudCallout", "Thought Bubble: Cloud", Callouts, [-20833, 62500]),
    p!("borderCallout1", "Callout: Line", Callouts, [18750, -8333, 112500, -38333]),
    p!("borderCallout2", "Callout: Bent Line", Callouts, [18750, -8333, 18750, -16667, 112500, -46667]),
    p!("borderCallout3", "Callout: Double Bent Line", Callouts, [18750, -8333, 18750, -16667, 100000, -16667, 112963, -8333]),
    p!("accentCallout1", "Callout: Line with Accent Bar", Callouts, [18750, -8333, 112500, -38333]),
    p!("accentCallout2", "Callout: Bent Line with Accent Bar", Callouts, [18750, -8333, 18750, -16667, 112500, -46667]),
    p!("accentCallout3", "Callout: Double Bent Line with Accent Bar", Callouts, [18750, -8333, 18750, -16667, 100000, -16667, 112963, -8333]),
    p!("callout1", "Callout: Line with No Border", Callouts, [18750, -8333, 112500, -38333]),
    p!("callout2", "Callout: Bent Line with No Border", Callouts, [18750, -8333, 18750, -16667, 112500, -46667]),
    p!("callout3", "Callout: Double Bent Line with No Border", Callouts, [18750, -8333, 18750, -16667, 100000, -16667, 112963, -8333]),
    p!("accentBorderCallout1", "Callout: Line with Border and Accent Bar", Callouts, [18750, -8333, 112500, -38333]),
    p!("accentBorderCallout2", "Callout: Bent Line with Border and Accent Bar", Callouts, [18750, -8333, 18750, -16667, 112500, -46667]),
    p!(
        "accentBorderCallout3",
        "Callout: Double Bent Line with Border and Accent Bar",
        Callouts,
        [18750, -8333, 18750, -16667, 100000, -16667, 112963, -8333]
    ),
    p!("actionButtonBackPrevious", "Action Button: Go Back or Previous", ActionButtons, []),
    p!("actionButtonForwardNext", "Action Button: Go Forward or Next", ActionButtons, []),
    p!("actionButtonBeginning", "Action Button: Go to Beginning", ActionButtons, []),
    p!("actionButtonEnd", "Action Button: Go to End", ActionButtons, []),
    p!("actionButtonHome", "Action Button: Go Home", ActionButtons, []),
    p!("actionButtonInformation", "Action Button: Get Information", ActionButtons, []),
    p!("actionButtonReturn", "Action Button: Go Back", ActionButtons, []),
    p!("actionButtonMovie", "Action Button: Video", ActionButtons, []),
    p!("actionButtonDocument", "Action Button: Document", ActionButtons, []),
    p!("actionButtonSound", "Action Button: Sound", ActionButtons, []),
    p!("actionButtonHelp", "Action Button: Help", ActionButtons, []),
    p!("actionButtonBlank", "Action Button: Custom", ActionButtons, []),
];

pub fn info(name: &str) -> Option<&'static PresetInfo> {
    CATALOG.iter().find(|p| p.name == name)
}

/// Whether PowerPoint joins this preset's outline with sharp (miter) corners when the line names
/// no join (it rounds them otherwise). Observed per preset on its export: the plus, the right,
/// up and down block arrows (not the left one), chevrons, stars from five points up, the
/// equation shapes, curved ribbons and the action buttons are mitred.
pub fn mitred_by_default(name: &str) -> bool {
    matches!(
        name,
        "plus"
            | "rightArrow"
            | "upArrow"
            | "downArrow"
            | "upDownArrow"
            | "leftUpArrow"
            | "bentUpArrow"
            | "stripedRightArrow"
            | "notchedRightArrow"
            | "chevron"
            | "leftArrowCallout"
            | "leftRightArrowCallout"
            | "upDownArrowCallout"
            | "quadArrowCallout"
            | "mathPlus"
            | "mathMinus"
            | "mathMultiply"
            | "mathEqual"
            | "mathNotEqual"
            | "star5"
            | "star6"
            | "star8"
            | "star10"
            | "star12"
            | "star16"
            | "star24"
            | "star32"
            | "ellipseRibbon"
            | "ellipseRibbon2"
    ) || name.starts_with("actionButton")
}

pub fn is_line_like(name: &str) -> bool {
    matches!(name, "line" | "straightConnector1" | "bentConnector2" | "bentConnector3" | "curvedConnector3" | "arc")
        || name.starts_with("bentConnector")
        || name.starts_with("curvedConnector")
}

// ---------- path builder ----------

struct Pb {
    p: BezPath,
    cur: Point,
}

impl Pb {
    fn new() -> Self {
        Pb { p: BezPath::new(), cur: Point::ZERO }
    }
    fn m(&mut self, x: f64, y: f64) -> &mut Self {
        self.cur = Point::new(x, y);
        self.p.move_to(self.cur);
        self
    }
    fn l(&mut self, x: f64, y: f64) -> &mut Self {
        self.cur = Point::new(x, y);
        self.p.line_to(self.cur);
        self
    }
    fn c(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) -> &mut Self {
        self.cur = Point::new(x, y);
        self.p.curve_to(Point::new(x1, y1), Point::new(x2, y2), self.cur);
        self
    }
    fn q(&mut self, x1: f64, y1: f64, x: f64, y: f64) -> &mut Self {
        self.cur = Point::new(x, y);
        self.p.quad_to(Point::new(x1, y1), self.cur);
        self
    }
    /// Elliptic arc continuing from the current point, which lies on the ellipse at parametric
    /// angle `st` (degrees); sweeps `sw` degrees (positive = clockwise on screen).
    fn arc(&mut self, wr: f64, hr: f64, st: f64, sw: f64) -> &mut Self {
        let (s, e) = (st.to_radians(), (st + sw).to_radians());
        let c = Point::new(self.cur.x - wr * s.cos(), self.cur.y - hr * s.sin());
        let a = Arc::new(c, Vec2::new(wr.abs(), hr.abs()), s, e - s, 0.0);
        a.to_cubic_beziers(0.05, |p1, p2, p| {
            self.p.curve_to(p1, p2, p);
        });
        self.cur = Point::new(c.x + wr * e.cos(), c.y + hr * e.sin());
        self
    }
    /// A DrawingML `arcTo`: angles are where the ray from the ellipse's centre at that angle
    /// meets it (not the ellipse's parameter), so elliptical arcs start and end where the
    /// standard's shapes expect.
    fn arc_spec(&mut self, wr: f64, hr: f64, st: f64, sw: f64) -> &mut Self {
        let par = |a: f64| {
            let a = a.to_radians();
            (wr * a.sin()).atan2(hr * a.cos())
        };
        let t0 = par(st);
        let mut d = par(st + sw) - t0;
        let tau = std::f64::consts::TAU;
        if sw.abs() >= 360.0 {
            d = tau.copysign(sw);
        } else {
            while sw > 0.0 && d < 0.0 {
                d += tau;
            }
            while sw < 0.0 && d > 0.0 {
                d -= tau;
            }
        }
        self.arc(wr, hr, t0.to_degrees(), d.to_degrees())
    }
    fn z(&mut self) -> &mut Self {
        self.p.close_path();
        self
    }
    fn poly(&mut self, pts: &[(f64, f64)]) -> &mut Self {
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                self.m(*x, *y);
            } else {
                self.l(*x, *y);
            }
        }
        self.z()
    }
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64) -> &mut Self {
        self.m(cx + rx, cy);
        self.arc(rx, ry, 0.0, 360.0);
        self.z()
    }
    fn take(&mut self) -> BezPath {
        std::mem::take(&mut self.p)
    }
}

fn fill(p: BezPath) -> SubPath {
    SubPath { path: p, fill: FillMode::Norm, stroke: true, even_odd: false }
}
fn fill_eo(p: BezPath) -> SubPath {
    SubPath { path: p, fill: FillMode::Norm, stroke: true, even_odd: true }
}
fn shade(p: BezPath, m: FillMode) -> SubPath {
    SubPath { path: p, fill: m, stroke: true, even_odd: false }
}
fn stroke_only(p: BezPath) -> SubPath {
    SubPath { path: p, fill: FillMode::None, stroke: true, even_odd: false }
}
fn fill_only(p: BezPath) -> SubPath {
    SubPath { path: p, fill: FillMode::Norm, stroke: false, even_odd: false }
}

/// Parametric angle on an rx×ry ellipse for a visual angle `deg` (file angles are visual).
fn vis(deg: f64, rx: f64, ry: f64) -> f64 {
    let t = deg.to_radians();
    (rx * t.sin()).atan2(ry * t.cos()).to_degrees()
}

fn polygon_regular(n: usize, w: f64, h: f64, phase: f64) -> Vec<(f64, f64)> {
    let pts: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = (phase + 360.0 * i as f64 / n as f64).to_radians();
            (a.cos(), a.sin())
        })
        .collect();
    normalize(&pts, w, h)
}

/// Scale `pts` so their bounding box fills 0..w × 0..h.
fn normalize(pts: &[(f64, f64)], w: f64, h: f64) -> Vec<(f64, f64)> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in pts {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    let sx = if x1 - x0 > 1e-12 { w / (x1 - x0) } else { 0.0 };
    let sy = if y1 - y0 > 1e-12 { h / (y1 - y0) } else { 0.0 };
    pts.iter().map(|(x, y)| ((x - x0) * sx, (y - y0) * sy)).collect()
}

/// An n-pointed star as ECMA-376 draws it: points on an ellipse around (hc, vc · vf) with radii
/// half the box stretched by the preset's `hf` / `vf` (so a five-pointed star fills its box and
/// looks regular in a square), the inner corners `inner` of the way out.
fn star(n: usize, w: f64, h: f64, inner: f64) -> Vec<(f64, f64)> {
    let (hf, vf) = match n {
        5 => (1.05146, 1.10557),
        6 => (1.15470, 1.0),
        7 => (1.02572, 1.05210),
        10 => (1.05146, 1.0),
        _ => (1.0, 1.0),
    };
    let (rx, ry, cy) = (w / 2.0 * hf, h / 2.0 * vf, h / 2.0 * vf);
    let mut pts = Vec::with_capacity(n * 2);
    for i in 0..n * 2 {
        let a = (-90.0 + 180.0 * i as f64 / n as f64).to_radians();
        let r = if i % 2 == 0 { 1.0 } else { inner };
        pts.push((w / 2.0 + r * rx * a.cos(), cy + r * ry * a.sin()));
    }
    pts
}

/// The text rectangle ECMA-376's presetShapeDefinitions give a star (`inner` is the adjust value
/// over 50000), where known: star4's is the square inside its inner points, star5's spans the
/// inner pentagon from its upper side points to its bottom inner point.
fn star_text_rect(n: usize, w: f64, h: f64, inner: f64) -> Option<Rect> {
    let (hc, vc) = (w / 2.0, h / 2.0);
    match n {
        4 => {
            let (dx, dy) = (hc * inner * 45f64.to_radians().cos(), vc * inner * 45f64.to_radians().sin());
            Some(Rect::new(hc - dx, vc - dy, hc + dx, vc + dy))
        }
        5 => {
            let (swd2, shd2, svc) = (hc * 1.05146, vc * 1.10557, vc * 1.10557);
            let (iwd2, ihd2) = (swd2 * inner, shd2 * inner);
            let sdx1 = iwd2 * 18f64.to_radians().cos();
            let sdy1 = ihd2 * 54f64.to_radians().sin();
            Some(Rect::new(hc - sdx1, svc - sdy1, hc + sdx1, svc + ihd2))
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum Corner {
    Square,
    Round(f64),
    Snip(f64),
}

/// A rectangle with each corner (TL, TR, BR, BL) square, rounded or snipped.
fn corner_rect(w: f64, h: f64, c: [Corner; 4]) -> BezPath {
    let mut b = Pb::new();
    let size = |k: Corner| match k {
        Corner::Square => 0.0,
        Corner::Round(r) | Corner::Snip(r) => r.max(0.0),
    };
    let s0 = size(c[0]);
    b.m(s0, 0.0);
    // top edge → TR
    let s1 = size(c[1]);
    b.l(w - s1, 0.0);
    match c[1] {
        Corner::Round(r) if r > 0.0 => {
            b.arc(r, r, 270.0, 90.0);
        }
        Corner::Snip(_) => {
            b.l(w, s1);
        }
        _ => {}
    }
    let s2 = size(c[2]);
    b.l(w, h - s2);
    match c[2] {
        Corner::Round(r) if r > 0.0 => {
            b.arc(r, r, 0.0, 90.0);
        }
        Corner::Snip(_) => {
            b.l(w - s2, h);
        }
        _ => {}
    }
    let s3 = size(c[3]);
    b.l(s3, h);
    match c[3] {
        Corner::Round(r) if r > 0.0 => {
            b.arc(r, r, 90.0, 90.0);
        }
        Corner::Snip(_) => {
            b.l(0.0, h - s3);
        }
        _ => {}
    }
    b.l(0.0, s0);
    match c[0] {
        Corner::Round(r) if r > 0.0 => {
            b.arc(r, r, 180.0, 90.0);
        }
        Corner::Snip(_) => {
            b.l(s0, 0.0);
        }
        _ => {}
    }
    b.z();
    b.take()
}

fn mid_sites(w: f64, h: f64) -> Vec<Point> {
    vec![Point::new(w / 2.0, 0.0), Point::new(0.0, h / 2.0), Point::new(w / 2.0, h), Point::new(w, h / 2.0)]
}

/// Orientation for single-axis arrows built along +x.
#[derive(Clone, Copy)]
enum Dir {
    Right,
    Left,
    Up,
    Down,
}

/// Map canonical (u along the arrow's length L, v across thickness T) to shape space.
fn orient(d: Dir, w: f64, h: f64, pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    pts.iter()
        .map(|&(u, v)| match d {
            Dir::Right => (u, v),
            Dir::Left => (w - u, v),
            Dir::Up => (v, h - u),
            Dir::Down => (v, u),
        })
        .collect()
}

fn len_thick(d: Dir, w: f64, h: f64) -> (f64, f64) {
    match d {
        Dir::Right | Dir::Left => (w, h),
        Dir::Up | Dir::Down => (h, w),
    }
}

fn arrow(d: Dir, w: f64, h: f64, a1: f64, a2: f64) -> Geometry {
    let (l, t) = len_thick(d, w, h);
    let ss = l.min(t);
    let a1 = a1.clamp(0.0, 100000.0) / 100000.0;
    let dx = (ss * a2.clamp(0.0, 100000.0 * l / ss.max(1e-9)) / 100000.0).min(l);
    let y1 = t / 2.0 - t * a1 / 2.0;
    let y2 = t / 2.0 + t * a1 / 2.0;
    let pts = [(0.0, y1), (l - dx, y1), (l - dx, 0.0), (l, t / 2.0), (l - dx, t), (l - dx, y2), (0.0, y2)];
    let pts = orient(d, w, h, &pts);
    let tr = orient(d, w, h, &[(0.0, y1), (l - dx + dx * (y1 / (t / 2.0).max(1e-9)), y2)]);
    let mut text = Rect::from_points(tr[0], tr[1]);
    text = text.abs();
    let mut g = Geometry { paths: vec![fill(Pb::new().poly(&pts).take())], text_rect: text, handles: vec![], sites: mid_sites(w, h) };
    // handle 0: shaft thickness on the head base; handle 1: head length.
    let o = orient(d, w, h, &[(l - dx, t / 2.0), (l, t / 2.0)]);
    let (o0, o1) = (Point::new(o[0].0, o[0].1), Point::new(o[1].0, o[1].1));
    let across = match d {
        Dir::Right | Dir::Left => Vec2::new(0.0, -t / 2.0 / 100000.0),
        Dir::Up | Dir::Down => Vec2::new(-t / 2.0 / 100000.0, 0.0),
    };
    let along = (o0 - o1) * (ss / 100000.0 / dx.max(1e-9));
    g.handles.push(Handle::new(0, o0, across, a1 * 100000.0, 0.0, 100000.0));
    if dx > 1e-9 {
        g.handles.push(Handle::new(1, o1, along, a2, 0.0, 100000.0 * l / ss.max(1e-9)));
    }
    g
}

fn double_arrow(vertical: bool, w: f64, h: f64, a1: f64, a2: f64) -> Geometry {
    let (l, t) = if vertical { (h, w) } else { (w, h) };
    let ss = l.min(t);
    let a1 = a1.clamp(0.0, 100000.0) / 100000.0;
    let dx = (ss * a2.max(0.0) / 100000.0).min(l / 2.0);
    let y1 = t / 2.0 - t * a1 / 2.0;
    let y2 = t / 2.0 + t * a1 / 2.0;
    let pts = [(0.0, t / 2.0), (dx, 0.0), (dx, y1), (l - dx, y1), (l - dx, 0.0), (l, t / 2.0), (l - dx, t), (l - dx, y2), (dx, y2), (dx, t)];
    let pts = orient(if vertical { Dir::Down } else { Dir::Right }, w, h, &pts);
    let r = if vertical { Rect::new(y1, dx, y2, h - dx) } else { Rect::new(dx, y1, w - dx, y2) };
    Geometry { paths: vec![fill(Pb::new().poly(&pts).take())], text_rect: r, handles: vec![], sites: mid_sites(w, h) }
}

/// Points given in 21600ths of the box (as ECMA-376 defines many fixed shapes), scaled to it.
fn spec_pts(pts: &[(i32, i32)], w: f64, h: f64) -> Vec<(f64, f64)> {
    pts.iter().map(|(x, y)| (w * *x as f64 / 21600.0, h * *y as f64 / 21600.0)).collect()
}

fn simple(path: BezPath, text: Rect, w: f64, h: f64) -> Geometry {
    Geometry { paths: vec![fill(path)], text_rect: text, handles: vec![], sites: mid_sites(w, h) }
}

/// Builds the geometry for preset `name` at size `w`×`h` with adjust values `adj` (missing
/// values use the preset's defaults). Unknown names return `None`; the caller falls back to `rect`.
pub fn build(name: &str, w: f64, h: f64, adj: &[f64]) -> Option<Geometry> {
    if !(w.is_finite() && h.is_finite()) {
        return None;
    }
    let w = w.max(0.0);
    let h = h.max(0.0);
    let defaults = info(name).map(|i| i.defaults).unwrap_or(&[]);
    let a = |i: usize| -> f64 {
        let v = adj.get(i).copied().or_else(|| defaults.get(i).copied()).unwrap_or(0.0);
        if v.is_finite() { v } else { 0.0 }
    };
    let ss = w.min(h);
    let full = Rect::new(0.0, 0.0, w, h);
    let mut b = Pb::new();
    let g = match name {
        "rect" | "textBox" | "flowChartProcess" => simple(corner_rect(w, h, [Corner::Square; 4]), full, w, h),
        "actionButtonBlank"
        | "actionButtonBackPrevious"
        | "actionButtonForwardNext"
        | "actionButtonBeginning"
        | "actionButtonEnd"
        | "actionButtonHome"
        | "actionButtonInformation"
        | "actionButtonReturn"
        | "actionButtonMovie"
        | "actionButtonDocument"
        | "actionButtonSound"
        | "actionButtonHelp" => action_button(name, w, h),
        "roundRect" => {
            let v = a(0).clamp(0.0, 50000.0);
            let r = ss * v / 100000.0;
            let mut g = simple(corner_rect(w, h, [Corner::Round(r); 4]), full.inset(-r * 0.29289), w, h);
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "flowChartAlternateProcess" => simple(corner_rect(w, h, [Corner::Round(ss / 6.0); 4]), full.inset(-ss / 6.0 * 0.29289), w, h),
        "snip1Rect" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let mut g = simple(
                corner_rect(w, h, [Corner::Square, Corner::Snip(d), Corner::Square, Corner::Square]),
                Rect::new(0.0, d / 2.0, w - d / 2.0, h),
                w,
                h,
            );
            g.handles.push(Handle::new(0, Point::new(w, 0.0), Vec2::new(-ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "snip2SameRect" => {
            let (d1, d2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(corner_rect(w, h, [Corner::Snip(d1), Corner::Snip(d1), Corner::Snip(d2), Corner::Snip(d2)]), full.inset(-d1.max(d2) / 2.0), w, h)
        }
        "snip2DiagRect" => {
            let (d1, d2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(corner_rect(w, h, [Corner::Snip(d1), Corner::Snip(d2), Corner::Snip(d1), Corner::Snip(d2)]), full.inset(-d1.max(d2) / 2.0), w, h)
        }
        "snipRoundRect" => {
            let (r, d) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(corner_rect(w, h, [Corner::Round(r), Corner::Snip(d), Corner::Square, Corner::Square]), full.inset(-r.max(d) / 2.0), w, h)
        }
        "round1Rect" => {
            let r = ss * a(0).clamp(0.0, 50000.0) / 1e5;
            simple(corner_rect(w, h, [Corner::Square, Corner::Round(r), Corner::Square, Corner::Square]), full.inset(-r * 0.29289), w, h)
        }
        "round2SameRect" => {
            let (r1, r2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(
                corner_rect(w, h, [Corner::Round(r1), Corner::Round(r1), Corner::Round(r2), Corner::Round(r2)]),
                full.inset(-r1.max(r2) * 0.29289),
                w,
                h,
            )
        }
        "round2DiagRect" => {
            let (r1, r2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(
                corner_rect(w, h, [Corner::Round(r1), Corner::Round(r2), Corner::Round(r1), Corner::Round(r2)]),
                full.inset(-r1.max(r2) * 0.29289),
                w,
                h,
            )
        }
        "ellipse" | "flowChartConnector" => {
            let mut g =
                simple(b.ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0).take(), Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355), w, h);
            g.sites = ellipse_sites(w, h);
            g
        }
        "triangle" | "flowChartExtract" => {
            let v = if name == "triangle" { a(0).clamp(0.0, 100000.0) } else { 50000.0 };
            let ax = w * v / 100000.0;
            let mut g = simple(b.poly(&[(0.0, h), (ax, 0.0), (w, h)]).take(), Rect::new(ax / 2.0, h / 2.0, (ax + w) / 2.0, h), w, h);
            g.sites = vec![
                Point::new(ax, 0.0),
                Point::new(ax / 2.0, h / 2.0),
                Point::new(0.0, h),
                Point::new(w / 2.0, h),
                Point::new(w, h),
                Point::new((ax + w) / 2.0, h / 2.0),
            ];
            if name == "triangle" {
                g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(w / 100000.0, 0.0), v, 0.0, 100000.0));
            }
            g
        }
        "flowChartMerge" => simple(b.poly(&[(0.0, 0.0), (w, 0.0), (w / 2.0, h)]).take(), Rect::new(w / 4.0, 0.0, w * 0.75, h / 2.0), w, h),
        "rtTriangle" => {
            simple(b.poly(&[(0.0, 0.0), (0.0, h), (w, h)]).take(), Rect::new(w / 12.0, h * 7.0 / 12.0, w * 7.0 / 12.0, h * 11.0 / 12.0), w, h)
        }
        "parallelogram" | "flowChartInputOutput" => {
            let v = if name == "parallelogram" { a(0).clamp(0.0, 100000.0 * w / ss.max(1e-9)) } else { 20000.0 * w / ss.max(1e-9) };
            let off = ss * v / 100000.0;
            let mut g = simple(
                b.poly(&[(0.0, h), (off, 0.0), (w, 0.0), (w - off, h)]).take(),
                Rect::new(off / 2.0 + off * 0.2, h * 0.08, w - off / 2.0 - off * 0.2, h * 0.92),
                w,
                h,
            );
            if name == "parallelogram" {
                g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 100000.0 * w / ss.max(1e-9)));
            }
            g
        }
        "trapezoid" => {
            let v = a(0).clamp(0.0, 50000.0 * w / ss.max(1e-9));
            let off = ss * v / 100000.0;
            let mut g =
                simple(b.poly(&[(0.0, h), (off, 0.0), (w - off, 0.0), (w, h)]).take(), Rect::new(off * 0.66, h * 0.2, w - off * 0.66, h), w, h);
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0 * w / ss.max(1e-9)));
            g
        }
        "flowChartManualOperation" => {
            simple(b.poly(&[(0.0, 0.0), (w, 0.0), (w * 0.8, h), (w * 0.2, h)]).take(), Rect::new(w * 0.2, 0.0, w * 0.8, h), w, h)
        }
        "diamond" | "flowChartDecision" => simple(
            b.poly(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]).take(),
            Rect::new(w / 4.0, h / 4.0, w * 0.75, h * 0.75),
            w,
            h,
        ),
        "flowChartSort" => {
            let d = b.poly(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]).take();
            let line = Pb::new().m(0.0, h / 2.0).l(w, h / 2.0).take();
            Geometry {
                paths: vec![fill(d), stroke_only(line)],
                text_rect: Rect::new(w / 4.0, h / 4.0, w * 0.75, h * 0.75),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "pentagon" => simple(b.poly(&polygon_regular(5, w, h, -90.0)).take(), Rect::new(w * 0.2, h * 0.27, w * 0.8, h * 0.9), w, h),
        "heptagon" => simple(b.poly(&polygon_regular(7, w, h, -90.0)).take(), Rect::new(w * 0.16, h * 0.2, w * 0.84, h * 0.86), w, h),
        "decagon" => simple(b.poly(&polygon_regular(10, w, h, 0.0)).take(), Rect::new(w * 0.1, h * 0.17, w * 0.9, h * 0.83), w, h),
        "dodecagon" => simple(b.poly(&polygon_regular(12, w, h, 15.0)).take(), Rect::new(w * 0.13, h * 0.13, w * 0.87, h * 0.87), w, h),
        "hexagon" | "flowChartPreparation" => {
            let v = if name == "hexagon" { a(0).clamp(0.0, 50000.0 * w / ss.max(1e-9)) } else { 20000.0 * w / ss.max(1e-9) };
            let off = ss * v / 100000.0;
            let mut g = simple(
                b.poly(&[(0.0, h / 2.0), (off, 0.0), (w - off, 0.0), (w, h / 2.0), (w - off, h), (off, h)]).take(),
                Rect::new(off * 0.6, h * 0.14, w - off * 0.6, h * 0.86),
                w,
                h,
            );
            if name == "hexagon" {
                g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0 * w / ss.max(1e-9)));
            }
            g
        }
        "octagon" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let mut g = simple(
                b.poly(&[(d, 0.0), (w - d, 0.0), (w, d), (w, h - d), (w - d, h), (d, h), (0.0, h - d), (0.0, d)]).take(),
                full.inset(-d / 2.0),
                w,
                h,
            );
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "plus" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let mut g = simple(
                b.poly(&[
                    (d, 0.0),
                    (w - d, 0.0),
                    (w - d, d),
                    (w, d),
                    (w, h - d),
                    (w - d, h - d),
                    (w - d, h),
                    (d, h),
                    (d, h - d),
                    (0.0, h - d),
                    (0.0, d),
                    (d, d),
                ])
                .take(),
                Rect::new(0.0, d, w, h - d),
                w,
                h,
            );
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "frame" => {
            let v = a(0).clamp(0.0, 50000.0);
            let t = ss * v / 100000.0;
            b.poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]);
            b.poly(&[(t, t), (w - t, t), (w - t, h - t), (t, h - t)]);
            let mut g = Geometry { paths: vec![fill_eo(b.take())], text_rect: full.inset(-t), handles: vec![], sites: mid_sites(w, h) };
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "halfFrame" => {
            let (t1, t2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(b.poly(&[(0.0, 0.0), (w, 0.0), (w - t1, t1), (t2, t1), (t2, h - t2), (0.0, h)]).take(), full, w, h)
        }
        "corner" => {
            let (t1, t2) = (ss * a(0).clamp(0.0, 100000.0) / 1e5, ss * a(1).clamp(0.0, 100000.0) / 1e5);
            simple(b.poly(&[(0.0, 0.0), (t2, 0.0), (t2, h - t1), (w, h - t1), (w, h), (0.0, h)]).take(), Rect::new(0.0, h - t1, w, h), w, h)
        }
        "diagStripe" => {
            let k = a(0).clamp(0.0, 100000.0) / 1e5;
            simple(b.poly(&[(0.0, h * k), (w * k, 0.0), (w, 0.0), (0.0, h)]).take(), Rect::new(0.0, 0.0, w / 2.0, h / 2.0), w, h)
        }
        "plaque" => {
            let v = a(0).clamp(0.0, 50000.0);
            let r = ss * v / 100000.0;
            b.m(0.0, r)
                .arc(r, r, 90.0, -90.0)
                .l(w - r, 0.0)
                .arc(r, r, 180.0, -90.0)
                .l(w, h - r)
                .arc(r, r, 270.0, -90.0)
                .l(r, h)
                .arc(r, r, 0.0, -90.0)
                .z();
            let mut g = simple(b.take(), full.inset(-r * 0.7), w, h);
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "can" | "flowChartMagneticDisk" => {
            // The disk's ends are a third of its height (ECMA-376: ellipses of radius h/6).
            let v = if name == "can" { a(0).clamp(0.0, 50000.0 * h / ss.max(1e-9)) } else { 100000.0 / 3.0 * h / ss.max(1e-9) };
            let eh = ss * v / 100000.0;
            let r = eh / 2.0;
            let body = Pb::new().m(0.0, r).l(0.0, h - r).arc(w / 2.0, r, 180.0, -180.0).l(w, r).arc(w / 2.0, r, 0.0, -180.0).z().take();
            let top = Pb::new().ellipse(w / 2.0, r, w / 2.0, r).take();
            let mut g = Geometry {
                paths: vec![fill(body), shade(top, if name == "can" { FillMode::Lighten } else { FillMode::Norm })],
                text_rect: Rect::new(0.0, eh, w, h - r),
                handles: vec![],
                sites: mid_sites(w, h),
            };
            if name == "can" {
                g.handles.push(Handle::new(0, Point::new(w / 2.0, 0.0), Vec2::new(0.0, ss / 100000.0), v, 0.0, 50000.0 * h / ss.max(1e-9)));
            }
            g
        }
        "flowChartMagneticDrum" => {
            // ECMA-376: end ellipses of radius w/6 across.
            let r = w / 6.0;
            let body = Pb::new().m(r, 0.0).l(w - r, 0.0).arc(r, h / 2.0, 270.0, 180.0).l(r, h).arc(r, h / 2.0, 90.0, 180.0).z().take();
            let side = Pb::new().m(w - r, 0.0).arc(r, h / 2.0, 270.0, -180.0).take();
            Geometry {
                paths: vec![fill(body), stroke_only(side)],
                text_rect: Rect::new(r, 0.0, w - 2.0 * r, h),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "cube" => {
            let v = a(0).clamp(0.0, 100000.0);
            let d = ss * v / 100000.0;
            let front = Pb::new().poly(&[(0.0, d), (w - d, d), (w - d, h), (0.0, h)]).take();
            let top = Pb::new().poly(&[(0.0, d), (d, 0.0), (w, 0.0), (w - d, d)]).take();
            let side = Pb::new().poly(&[(w - d, d), (w, 0.0), (w, h - d), (w - d, h)]).take();
            let mut g = Geometry {
                paths: vec![fill(front), shade(top, FillMode::LightenLess), shade(side, FillMode::DarkenLess)],
                text_rect: Rect::new(0.0, d, w - d, h),
                handles: vec![],
                sites: mid_sites(w, h),
            };
            g.handles.push(Handle::new(0, Point::new(0.0, 0.0), Vec2::new(0.0, ss / 100000.0), v, 0.0, 100000.0));
            g
        }
        "bevel" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let inner = Pb::new().poly(&[(d, d), (w - d, d), (w - d, h - d), (d, h - d)]).take();
            let top = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w - d, d), (d, d)]).take();
            let left = Pb::new().poly(&[(0.0, 0.0), (d, d), (d, h - d), (0.0, h)]).take();
            let right = Pb::new().poly(&[(w, 0.0), (w, h), (w - d, h - d), (w - d, d)]).take();
            let bottom = Pb::new().poly(&[(0.0, h), (d, h - d), (w - d, h - d), (w, h)]).take();
            let mut g = Geometry {
                paths: vec![
                    fill(inner),
                    shade(top, FillMode::LightenLess),
                    shade(left, FillMode::Lighten),
                    shade(right, FillMode::DarkenLess),
                    shade(bottom, FillMode::Darken),
                ],
                text_rect: full.inset(-d),
                handles: vec![],
                sites: mid_sites(w, h),
            };
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "donut" => {
            let v = a(0).clamp(0.0, 50000.0);
            let t = ss * v / 100000.0;
            b.ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0);
            b.ellipse(w / 2.0, h / 2.0, (w / 2.0 - t).max(0.0), (h / 2.0 - t).max(0.0));
            let mut g = Geometry {
                paths: vec![fill_eo(b.take())],
                text_rect: Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355),
                handles: vec![],
                sites: ellipse_sites(w, h),
            };
            g.handles.push(Handle::new(0, Point::new(0.0, h / 2.0), Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "noSmoking" => {
            let v = a(0).clamp(0.0, 50000.0);
            let t = ss * v / 100000.0;
            let (rx, ry) = ((w / 2.0 - t).max(0.0), (h / 2.0 - t).max(0.0));
            b.ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0);
            b.ellipse(w / 2.0, h / 2.0, rx, ry);
            let ring = b.take();
            let th = (h).atan2(w);
            let dir = Vec2::new(th.cos(), th.sin());
            let r = if rx > 0.0 && ry > 0.0 { rx * ry / ((ry * th.cos()).powi(2) + (rx * th.sin()).powi(2)).sqrt() } else { 0.0 };
            let perp = Vec2::new(-dir.y, dir.x) * (t / 2.0);
            let c = Point::new(w / 2.0, h / 2.0);
            let p = [c - dir * r - perp, c + dir * r - perp, c + dir * r + perp, c - dir * r + perp];
            let bar = Pb::new().poly(&p.map(|q| (q.x, q.y))).take();
            Geometry {
                paths: vec![fill_eo(ring), fill(bar)],
                text_rect: Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355),
                handles: vec![Handle::new(0, Point::new(0.0, h / 2.0), Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0)],
                sites: ellipse_sites(w, h),
            }
        }
        "blockArc" => {
            let st = a(0) / 60000.0;
            let en = a(1) / 60000.0;
            let t = ss * a(2).clamp(0.0, 50000.0) / 100000.0;
            let mut sw = en - st;
            if sw <= 0.0 {
                sw += 360.0;
            }
            let (rx, ry) = (w / 2.0, h / 2.0);
            let (irx, iry) = ((rx - t).max(0.0), (ry - t).max(0.0));
            let s0 = vis(st, rx, ry);
            let e0 = vis(st + sw, rx, ry);
            let mut sweep = e0 - s0;
            if sweep <= 0.0 {
                sweep += 360.0;
            }
            let si = vis(st + sw, irx, iry);
            let ei = vis(st, irx, iry);
            let mut isweep = ei - si;
            if isweep >= 0.0 {
                isweep -= 360.0;
            }
            let p0 = Point::new(w / 2.0 + rx * s0.to_radians().cos(), h / 2.0 + ry * s0.to_radians().sin());
            b.m(p0.x, p0.y).arc(rx, ry, s0, sweep);
            let pi = Point::new(w / 2.0 + irx * si.to_radians().cos(), h / 2.0 + iry * si.to_radians().sin());
            b.l(pi.x, pi.y).arc(irx, iry, si, isweep).z();
            let mut g = simple(b.take(), full, w, h);
            let c = Point::new(rx, ry);
            g.handles.push(Handle::angle(0, c, rx - t / 2.0, ry - t / 2.0, a(0)));
            // Thickness: slides inward from the outer edge along the end angle's ray.
            let pe = Point::new(rx + rx * e0.to_radians().cos(), ry + ry * e0.to_radians().sin());
            let u = (c - pe).normalize();
            let u = if u.x.is_finite() && u.y.is_finite() { u } else { Vec2::ZERO };
            g.handles.push(Handle::new(2, pe, u * (ss / 100000.0), a(2).clamp(0.0, 50000.0), 0.0, 50000.0));
            g
        }
        "pie" | "chord" | "arc" => {
            let (rx, ry) = (w / 2.0, h / 2.0);
            let s = vis(a(0) / 60000.0, rx, ry);
            let e = vis(a(1) / 60000.0, rx, ry);
            let mut sw = e - s;
            if sw <= 0.0 {
                sw += 360.0;
            }
            let p0 = Point::new(rx + rx * s.to_radians().cos(), ry + ry * s.to_radians().sin());
            b.m(p0.x, p0.y).arc(rx, ry, s, sw);
            let text = Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355);
            let mut g = match name {
                "pie" => {
                    b.l(rx, ry).z();
                    simple(b.take(), text, w, h)
                }
                "chord" => {
                    b.z();
                    simple(b.take(), text, w, h)
                }
                _ => {
                    let open = b.take();
                    let mut f = BezPath::from_vec(open.elements().to_vec());
                    f.line_to(Point::new(rx, ry));
                    f.close_path();
                    Geometry { paths: vec![fill_only(f), stroke_only(open)], text_rect: text, handles: vec![], sites: mid_sites(w, h) }
                }
            };
            let c = Point::new(rx, ry);
            g.handles.push(Handle::angle(0, c, rx, ry, a(0)));
            g.handles.push(Handle::angle(1, c, rx, ry, a(1)));
            g
        }
        "teardrop" => {
            let k = a(0).clamp(0.0, 200000.0) / 100000.0;
            let tip = (w / 2.0 + w / 2.0 * k, h / 2.0 - h / 2.0 * k);
            b.m(0.0, h / 2.0).arc(w / 2.0, h / 2.0, 180.0, 90.0);
            b.q(w / 2.0 + w / 4.0 * k, 0.0, tip.0, tip.1).q(w, h / 2.0 - h / 4.0 * k, w, h / 2.0);
            b.arc(w / 2.0, h / 2.0, 0.0, 180.0).z();
            simple(b.take(), Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355), w, h)
        }
        "foldedCorner" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let body = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w, h - d), (w - d, h), (0.0, h)]).take();
            let fold = Pb::new().m(w - d, h).l(w - d + d * 0.2, h - d + d * 0.2).l(w, h - d).z().take();
            Geometry {
                paths: vec![fill(body), shade(fold, FillMode::DarkenLess)],
                text_rect: Rect::new(0.0, 0.0, w, h - d),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "smileyFace" => {
            let k = a(0).clamp(-4653.0, 4653.0) / 100000.0;
            let face = Pb::new().ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0).take();
            let mut eyes = Pb::new();
            eyes.ellipse(w * 0.35, h * 0.37, w * 0.05, h * 0.05);
            eyes.ellipse(w * 0.65, h * 0.37, w * 0.05, h * 0.05);
            let ys = h * (0.72 - k);
            let smile = Pb::new().m(w * 0.22, ys).q(w / 2.0, ys + h * k * 4.0 + h * 0.0, w * 0.78, ys).take();
            Geometry {
                paths: vec![fill(face), shade(eyes.take(), FillMode::DarkenLess), stroke_only(smile)],
                text_rect: Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355),
                handles: vec![],
                sites: ellipse_sites(w, h),
            }
        }
        "heart" => {
            // ECMA-376 heart: two cubic curves from the notch at the top to the point.
            let (dx1, dx2) = (w * 49.0 / 48.0, w * 10.0 / 48.0);
            let y1 = -h / 3.0;
            b.m(w / 2.0, h / 4.0)
                .c(w / 2.0 + dx2, y1, w / 2.0 + dx1, h / 4.0, w / 2.0, h)
                .c(w / 2.0 - dx1, h / 4.0, w / 2.0 - dx2, y1, w / 2.0, h / 4.0)
                .z();
            simple(b.take(), Rect::new(w * 0.2, h * 0.25, w * 0.8, h * 0.7), w, h)
        }
        "lightningBolt" => {
            // ECMA-376 lightningBolt, in 21600ths of the box.
            let pts = spec_pts(
                &[
                    (8472, 0),
                    (12860, 6080),
                    (11050, 6797),
                    (16577, 12007),
                    (14767, 12877),
                    (21600, 21600),
                    (10012, 14915),
                    (12222, 13987),
                    (5022, 9705),
                    (7602, 8382),
                    (0, 3890),
                ],
                w,
                h,
            );
            simple(b.poly(&pts).take(), Rect::new(w * 8757.0 / 21600.0, h * 7637.0 / 21600.0, w * 13917.0 / 21600.0, h * 12963.0 / 21600.0), w, h)
        }
        "sun" => {
            // ECMA-376 sun: eight triangular rays (their bases at fixed fractions of the core's
            // margin) around a circle of radius 50% - adj.
            let a = a(0).clamp(12500.0, 46875.0);
            let g0 = 50000.0 - a;
            let (g1, g2) = (g0 * 30274.0 / 32768.0, g0 * 12540.0 / 32768.0);
            let (g10, g11) = ((50000.0 - g1) * 0.75, (50000.0 - g2) * 0.75);
            let (g12, g13, g14) = (g10 + 3662.0, g11 + 3662.0, g11 + 12500.0);
            let (g15, g16, g17, g18) = (100000.0 - g10, 100000.0 - g12, 100000.0 - g13, 100000.0 - g14);
            let x = |g: f64| w * g / 100000.0;
            let y = |g: f64| h * g / 100000.0;
            let (ox1, oy1, ox2, oy2) = (w * 18436.0 / 21600.0, h * 3163.0 / 21600.0, w * 3163.0 / 21600.0, h * 18436.0 / 21600.0);
            let (hc, vc) = (w / 2.0, h / 2.0);
            b.poly(&[(w, vc), (x(g15), y(g18)), (x(g15), y(g14))]);
            b.poly(&[(ox1, oy1), (x(g16), y(g13)), (x(g17), y(g12))]);
            b.poly(&[(hc, 0.0), (x(g18), y(g10)), (x(g14), y(g10))]);
            b.poly(&[(ox2, oy1), (x(g13), y(g12)), (x(g12), y(g13))]);
            b.poly(&[(0.0, vc), (x(g10), y(g14)), (x(g10), y(g18))]);
            b.poly(&[(ox2, oy2), (x(g12), y(g17)), (x(g13), y(g16))]);
            b.poly(&[(hc, h), (x(g14), y(g15)), (x(g18), y(g15))]);
            b.poly(&[(ox1, oy2), (x(g17), y(g16)), (x(g16), y(g17))]);
            b.ellipse(hc, vc, x(g0), y(g0));
            let g7 = g0 * 23170.0 / 32768.0;
            let mut g = simple(b.take(), Rect::new(x(50000.0 - g7), y(50000.0 - g7), x(50000.0 + g7), y(50000.0 + g7)), w, h);
            g.handles.push(Handle::new(0, Point::new(0.0, vc), Vec2::new(w / 100000.0, 0.0), a, 12500.0, 46875.0));
            g
        }
        "moon" => {
            // ECMA-376 moon: the outer edge is the left half of an ellipse w × h/2 centred on the
            // right edge; the inner edge an arc through the corners and (adj · w, middle) of a
            // larger ellipse (rx = g18, ry = dy1) whose centre lies right of that point.
            let k = a(0).clamp(0.0, 87500.0) / 100000.0;
            let ssf = ss.max(1e-9);
            let g0 = ss * k;
            let g1 = (ss - g0).max(1e-9);
            let g2 = g0 * g0 / g1;
            let g3 = ss * ss / g1;
            let g5 = 2.0 * g3 - g2;
            let g6 = g5 - g0;
            let dy1 = (g5 / 2.0 - g0) * (h / 2.0) / ssf;
            let (g0w, g6w) = (g0 * w / ssf, g6 * w / ssf);
            let rx = (g6w - g0w) / 2.0;
            let cx = g0w + rx;
            b.m(w, 0.0).arc(w, h / 2.0, 270.0, -180.0);
            if rx > 1e-9 && dy1 > 1e-9 {
                // From the bottom-right corner back up to the top-right through (g0w, h/2).
                let t0 = ((h / 2.0) / dy1).clamp(-1.0, 1.0).atan2((w - cx) / rx);
                let t1 = (-(h / 2.0) / dy1).clamp(-1.0, 1.0).atan2((w - cx) / rx) + std::f64::consts::TAU;
                let arc = Arc::new(Point::new(cx, h / 2.0), Vec2::new(rx, dy1), t0, t1 - t0, 0.0);
                arc.to_cubic_beziers(0.05, |p1, p2, p| {
                    b.p.curve_to(p1, p2, p);
                });
            }
            b.z();
            let g12 = g0 * 9598.0 / 32768.0;
            let g13 = ss - g12;
            let dy4 = (ss * ss - g13 * g13).max(0.0).sqrt() * (h / 2.0) / ssf;
            simple(b.take(), Rect::new(g12 * w / ssf, h / 2.0 - dy4, g0w, h / 2.0 + dy4), w, h)
        }
        "cloud" | "cloudCallout" => cloud(name, w, h, a(0), a(1)),
        "bracketPair" => {
            let r = ss * a(0).clamp(0.0, 50000.0) / 1e5;
            let mut l = Pb::new();
            l.m(r, h).arc(r, r, 90.0, 90.0).l(0.0, r).arc(r, r, 180.0, 90.0);
            l.m(w - r, 0.0).arc(r, r, 270.0, 90.0).l(w, h - r).arc(r, r, 0.0, 90.0);
            let f = corner_rect(w, h, [Corner::Round(r); 4]);
            Geometry {
                paths: vec![fill_only(f), stroke_only(l.take())],
                text_rect: full.inset(-r * 0.29289),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "bracePair" => {
            let r = ss * a(0).clamp(0.0, 25000.0) / 1e5;
            let mut l = Pb::new();
            l.m(2.0 * r, h).arc(r, r, 90.0, 90.0).l(r, h / 2.0 + r).arc(r, r, 0.0, -90.0).arc(r, r, 90.0, -90.0).l(r, r).arc(r, r, 180.0, 90.0);
            l.m(w - 2.0 * r, 0.0)
                .arc(r, r, 270.0, 90.0)
                .l(w - r, h / 2.0 - r)
                .arc(r, r, 180.0, -90.0)
                .arc(r, r, 270.0, -90.0)
                .l(w - r, h - r)
                .arc(r, r, 0.0, 90.0);
            let f = corner_rect(w, h, [Corner::Round(r); 4]);
            Geometry { paths: vec![fill_only(f), stroke_only(l.take())], text_rect: full.inset(-r * 1.2), handles: vec![], sites: mid_sites(w, h) }
        }
        "leftBracket" | "rightBracket" => {
            let r = (ss * a(0).clamp(0.0, 50000.0 * h / ss.max(1e-9)) / 1e5).min(h / 2.0);
            let mut l = Pb::new();
            l.m(w, h).arc(w, r, 90.0, 90.0).l(0.0, r).arc(w, r, 180.0, 90.0);
            let mut path = l.take();
            if name == "rightBracket" {
                path.apply_affine(kurbo::Affine::new([-1.0, 0.0, 0.0, 1.0, w, 0.0]));
            }
            // Filled (when the shape has a fill) out to the straight side, as PowerPoint does.
            let mut area = path.clone();
            area.close_path();
            Geometry { paths: vec![fill_only(area), stroke_only(path)], text_rect: full, handles: vec![], sites: mid_sites(w, h) }
        }
        "leftBrace" | "rightBrace" => {
            let r = (ss * a(0).clamp(0.0, 25000.0 * h / ss.max(1e-9)) / 1e5).min(h / 4.0);
            let my = h * a(1).clamp(0.0, 100000.0) / 1e5;
            let hw = w / 2.0;
            let mut l = Pb::new();
            l.m(w, h).arc(hw, r, 90.0, 90.0).l(hw, my + r).arc(hw, r, 0.0, -90.0).arc(hw, r, 90.0, -90.0).l(hw, r).arc(hw, r, 180.0, 90.0);
            let mut path = l.take();
            if name == "rightBrace" {
                path.apply_affine(kurbo::Affine::new([-1.0, 0.0, 0.0, 1.0, w, 0.0]));
            }
            // Filled (when the shape has a fill) out to the straight side, as PowerPoint does.
            let mut area = path.clone();
            area.close_path();
            Geometry { paths: vec![fill_only(area), stroke_only(path)], text_rect: full, handles: vec![], sites: mid_sites(w, h) }
        }
        "rightArrow" => arrow(Dir::Right, w, h, a(0), a(1)),
        "leftArrow" => arrow(Dir::Left, w, h, a(0), a(1)),
        "upArrow" => arrow(Dir::Up, w, h, a(0), a(1)),
        "downArrow" => arrow(Dir::Down, w, h, a(0), a(1)),
        "leftRightArrow" => double_arrow(false, w, h, a(0), a(1)),
        "upDownArrow" => double_arrow(true, w, h, a(0), a(1)),
        "notchedRightArrow" | "stripedRightArrow" => {
            let mut g = arrow(Dir::Right, w, h, a(0), a(1));
            let k = a(0).clamp(0.0, 100000.0) / 1e5;
            let (y1, y2) = (h / 2.0 - h * k / 2.0, h / 2.0 + h * k / 2.0);
            let dx = (ss * a(1).max(0.0) / 1e5).min(w);
            if name == "notchedRightArrow" {
                // x1 = dy1 · dx2 / hd2 in presetShapeDefinitions, with dy1 = h·k/2: the notch's
                // edges run parallel to the head's
                let n = dx * k;
                let pts = [(0.0, y1), (w - dx, y1), (w - dx, 0.0), (w, h / 2.0), (w - dx, h), (w - dx, y2), (0.0, y2), (n, h / 2.0)];
                g.paths = vec![fill(Pb::new().poly(&pts).take())];
            } else {
                let s = ss / 32.0;
                let start = 5.0 * s;
                let pts = [(start, y1), (w - dx, y1), (w - dx, 0.0), (w, h / 2.0), (w - dx, h), (w - dx, y2), (start, y2)];
                let mut stripes = Pb::new();
                stripes.poly(&[(0.0, y1), (s, y1), (s, y2), (0.0, y2)]);
                stripes.poly(&[(2.0 * s, y1), (4.0 * s, y1), (4.0 * s, y2), (2.0 * s, y2)]);
                g.paths = vec![fill(stripes.take()), fill(Pb::new().poly(&pts).take())];
            }
            g
        }
        "homePlate" | "chevron" => {
            let v = a(0).clamp(0.0, 100000.0 * w / ss.max(1e-9));
            let d = (ss * v / 1e5).min(w);
            let pts: Vec<(f64, f64)> = if name == "homePlate" {
                vec![(0.0, 0.0), (w - d, 0.0), (w, h / 2.0), (w - d, h), (0.0, h)]
            } else {
                vec![(0.0, 0.0), (w - d, 0.0), (w, h / 2.0), (w - d, h), (0.0, h), (d, h / 2.0)]
            };
            // Text rectangles as presetShapeDefinitions give them: homePlate's runs to halfway
            // along the point; chevron's lies between the notch and the point's base (x1..x2)
            // and covers the whole width once those meet or cross (`?: dx x1 l`).
            let text = if name == "homePlate" {
                Rect::new(0.0, 0.0, w - d / 2.0, h)
            } else if w - d > d {
                Rect::new(d, 0.0, w - d, h)
            } else {
                Rect::new(0.0, 0.0, w, h)
            };
            let mut g = simple(b.poly(&pts).take(), text, w, h);
            g.handles.push(Handle::new(0, Point::new(w, 0.0), Vec2::new(-ss / 1e5, 0.0), v, 0.0, 100000.0 * w / ss.max(1e-9)));
            g
        }
        "quadArrow" => {
            let (t, hw, hl) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5, ss * a(2).clamp(0.0, 50000.0) / 1e5);
            let (cx, cy) = (w / 2.0, h / 2.0);
            let (t2, hw2) = (t / 2.0, hw);
            let pts = [
                (0.0, cy),
                (hl, cy - hw2),
                (hl, cy - t2),
                (cx - t2, cy - t2),
                (cx - t2, hl),
                (cx - hw2, hl),
                (cx, 0.0),
                (cx + hw2, hl),
                (cx + t2, hl),
                (cx + t2, cy - t2),
                (w - hl, cy - t2),
                (w - hl, cy - hw2),
                (w, cy),
                (w - hl, cy + hw2),
                (w - hl, cy + t2),
                (cx + t2, cy + t2),
                (cx + t2, h - hl),
                (cx + hw2, h - hl),
                (cx, h),
                (cx - hw2, h - hl),
                (cx - t2, h - hl),
                (cx - t2, cy + t2),
                (hl, cy + t2),
                (hl, cy + hw2),
            ];
            simple(b.poly(&pts).take(), Rect::new(cx - t2, cy - t2, cx + t2, cy + t2), w, h)
        }
        "leftRightUpArrow" => {
            let (t, hw, hl) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5, ss * a(2).clamp(0.0, 50000.0) / 1e5);
            let cx = w / 2.0;
            let by = h - hw; // horizontal shaft centre
            let pts = [
                (0.0, by),
                (hl, by - hw),
                (hl, by - t / 2.0),
                (cx - t / 2.0, by - t / 2.0),
                (cx - t / 2.0, hl),
                (cx - hw, hl),
                (cx, 0.0),
                (cx + hw, hl),
                (cx + t / 2.0, hl),
                (cx + t / 2.0, by - t / 2.0),
                (w - hl, by - t / 2.0),
                (w - hl, by - hw),
                (w, by),
                (w - hl, h),
                (w - hl, by + t / 2.0),
                (hl, by + t / 2.0),
                (hl, h),
            ];
            simple(b.poly(&pts).take(), Rect::new(hl, by - t / 2.0, w - hl, by + t / 2.0), w, h)
        }
        "leftUpArrow" | "bentUpArrow" => {
            let (t, hw, hl) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5, ss * a(2).clamp(0.0, 50000.0) / 1e5);
            let pts: Vec<(f64, f64)> = if name == "leftUpArrow" {
                let cx = w - hw;
                let cy = h - hw;
                vec![
                    (0.0, cy),
                    (hl, cy - hw),
                    (hl, cy - t / 2.0),
                    (cx - t / 2.0, cy - t / 2.0),
                    (cx - t / 2.0, hl),
                    (cx - hw, hl),
                    (cx, 0.0),
                    (w, hl),
                    (cx + t / 2.0, hl),
                    (cx + t / 2.0, cy + t / 2.0),
                    (hl, cy + t / 2.0),
                    (hl, h),
                ]
            } else {
                let cx = w - hw;
                vec![
                    (0.0, h - t),
                    (cx - t / 2.0, h - t),
                    (cx - t / 2.0, hl),
                    (cx - hw, hl),
                    (cx, 0.0),
                    (w, hl),
                    (cx + t / 2.0, hl),
                    (cx + t / 2.0, h),
                    (0.0, h),
                ]
            };
            simple(b.poly(&pts).take(), full, w, h)
        }
        "bentArrow" => {
            let (t, hw, hl, r) = (
                ss * a(0).clamp(0.0, 50000.0) / 1e5,
                ss * a(1).clamp(0.0, 50000.0) / 1e5,
                ss * a(2).clamp(0.0, 50000.0) / 1e5,
                ss * a(3).clamp(0.0, 100000.0) / 1e5,
            );
            let cy = hw; // head centre line
            let ro = r.max(t);
            let ri = (ro - t).max(0.0);
            b.m(0.0, h)
                .l(0.0, cy - t / 2.0 + ro)
                .arc(ro, ro, 180.0, 90.0)
                .l(w - hl, cy - t / 2.0)
                .l(w - hl, 0.0)
                .l(w, cy)
                .l(w - hl, 2.0 * hw)
                .l(w - hl, cy + t / 2.0);
            b.l(t + ri, cy + t / 2.0);
            if ri > 0.0 {
                b.arc(ri, ri, 270.0, -90.0);
            }
            b.l(t, h).z();
            simple(b.take(), full, w, h)
        }
        "uturnArrow" => {
            let (t, hw, hl, r) = (
                ss * a(0).clamp(0.0, 25000.0) / 1e5,
                ss * a(1).clamp(0.0, 25000.0) / 1e5,
                ss * a(2).clamp(0.0, 25000.0) / 1e5,
                ss * a(3).clamp(0.0, 100000.0) / 1e5,
            );
            let hy = (h * a(4).clamp(0.0, 100000.0) / 1e5).max(hl + t);
            let ax = w - hw; // arrow shaft centre x
            let ro = r.max(t).min((ax + t / 2.0) / 2.0);
            let ri = (ro - t).max(0.0);
            b.m(0.0, h).l(0.0, ro).arc(ro, ro, 180.0, 90.0).l(ax + t / 2.0 - ro, 0.0).arc(ro, ro, 270.0, 90.0);
            b.l(ax + t / 2.0, hy - hl).l(w, hy - hl).l(ax, hy).l(ax - hw, hy - hl).l(ax - t / 2.0, hy - hl).l(ax - t / 2.0, ro);
            if ri > 0.0 {
                b.arc(ri, ri, 0.0, -90.0);
            }
            b.l(t + ri, t);
            if ri > 0.0 {
                b.arc(ri, ri, 270.0, -90.0);
            }
            b.l(t, h).z();
            simple(b.take(), full, w, h)
        }
        "rightArrowCallout" | "leftArrowCallout" | "upArrowCallout" | "downArrowCallout" => {
            let d = match name {
                "rightArrowCallout" => Dir::Right,
                "leftArrowCallout" => Dir::Left,
                "upArrowCallout" => Dir::Up,
                _ => Dir::Down,
            };
            let (l, t) = len_thick(d, w, h);
            let ss2 = l.min(t);
            let shaft = ss2 * a(0).clamp(0.0, 100000.0) / 1e5;
            let head = ss2 * a(1).clamp(0.0, 100000.0) / 1e5;
            let hl = ss2 * a(2).clamp(0.0, 100000.0) / 1e5;
            let boxl = (l * a(3).clamp(0.0, 100000.0) / 1e5).min(l - hl);
            let c = t / 2.0;
            let pts = [
                (0.0, 0.0),
                (boxl, 0.0),
                (boxl, c - shaft / 2.0),
                (l - hl, c - shaft / 2.0),
                (l - hl, c - head),
                (l, c),
                (l - hl, c + head),
                (l - hl, c + shaft / 2.0),
                (boxl, c + shaft / 2.0),
                (boxl, t),
                (0.0, t),
            ];
            let pts = orient(d, w, h, &pts);
            let tr = orient(d, w, h, &[(0.0, 0.0), (boxl, t)]);
            simple(b.poly(&pts).take(), Rect::from_points(Point::new(tr[0].0, tr[0].1), Point::new(tr[1].0, tr[1].1)).abs(), w, h)
        }
        "mathPlus" | "mathMinus" | "mathEqual" | "mathDivide" | "mathMultiply" | "mathNotEqual" => math(name, w, h, a(0), a(1), a(2)),
        "flowChartPredefinedProcess" => {
            let r = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]).take();
            let mut l = Pb::new();
            l.m(w / 8.0, 0.0).l(w / 8.0, h).m(w * 7.0 / 8.0, 0.0).l(w * 7.0 / 8.0, h);
            Geometry {
                paths: vec![fill(r), stroke_only(l.take())],
                text_rect: Rect::new(w / 8.0, 0.0, w * 7.0 / 8.0, h),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "flowChartInternalStorage" => {
            let r = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]).take();
            let mut l = Pb::new();
            l.m(w / 8.0, 0.0).l(w / 8.0, h).m(0.0, h / 8.0).l(w, h / 8.0);
            Geometry {
                paths: vec![fill(r), stroke_only(l.take())],
                text_rect: Rect::new(w / 8.0, h / 8.0, w, h),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "flowChartDocument" => {
            let u = |x: f64, y: f64| (w * x / 21600.0, h * y / 21600.0);
            let (p1, p2, p3, p4) = (u(21600.0, 17322.0), u(10800.0, 17322.0), u(10800.0, 23922.0), u(0.0, 20172.0));
            b.m(0.0, 0.0).l(w, 0.0).l(p1.0, p1.1).c(p2.0, p2.1, p3.0, p3.1, p4.0, p4.1).z();
            simple(b.take(), Rect::new(0.0, 0.0, w, h * 17322.0 / 21600.0), w, h)
        }
        "flowChartMultidocument" => {
            // ECMA-376 multidocument (21600ths): two back pages peeking out above and right of
            // the front one; the back pages' edges are outlined only where they show.
            let x = |v: f64| w * v / 21600.0;
            let y = |v: f64| h * v / 21600.0;
            let front = |b: &mut Pb| {
                b.m(0.0, y(20782.0))
                    .c(x(9298.0), y(23542.0), x(9298.0), y(18022.0), x(18595.0), y(18022.0))
                    .l(x(18595.0), y(3675.0))
                    .l(0.0, y(3675.0))
                    .z();
            };
            let mid = |b: &mut Pb| {
                b.m(x(1532.0), y(3675.0)).l(x(1532.0), y(1815.0)).l(x(20000.0), y(1815.0)).l(x(20000.0), y(16252.0));
                b.c(x(19298.0), y(16252.0), x(18595.0), y(16352.0), x(18595.0), y(16352.0));
            };
            let back = |b: &mut Pb| {
                b.m(x(2972.0), y(1815.0)).l(x(2972.0), 0.0).l(w, 0.0).l(w, y(14392.0));
                b.c(x(20800.0), y(14392.0), x(20000.0), y(14467.0), x(20000.0), y(14467.0));
            };
            let mut area = Pb::new();
            front(&mut area);
            mid(&mut area);
            area.l(x(18595.0), y(3675.0)).z();
            back(&mut area);
            area.l(x(20000.0), y(1815.0)).z();
            let mut outline = Pb::new();
            outline.m(0.0, y(3675.0)).l(x(18595.0), y(3675.0)).l(x(18595.0), y(18022.0));
            outline.c(x(9298.0), y(18022.0), x(9298.0), y(23542.0), 0.0, y(20782.0)).z();
            mid(&mut outline);
            back(&mut outline);
            Geometry {
                paths: vec![fill_only(area.take()), stroke_only(outline.take())],
                text_rect: Rect::new(0.0, y(3675.0), x(18595.0), y(17322.0)),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "flowChartTerminator" => {
            let rx = (w * 0.16).min(w / 2.0);
            b.m(rx, 0.0).l(w - rx, 0.0).arc(rx, h / 2.0, 270.0, 180.0).l(rx, h).arc(rx, h / 2.0, 90.0, 180.0).z();
            simple(b.take(), Rect::new(rx * 0.3, h * 0.15, w - rx * 0.3, h * 0.85), w, h)
        }
        "flowChartManualInput" => simple(b.poly(&[(0.0, h / 5.0), (w, 0.0), (w, h), (0.0, h)]).take(), Rect::new(0.0, h / 5.0, w, h), w, h),
        "flowChartOffpageConnector" => {
            simple(b.poly(&[(0.0, 0.0), (w, 0.0), (w, h * 0.8), (w / 2.0, h), (0.0, h * 0.8)]).take(), Rect::new(0.0, 0.0, w, h * 0.8), w, h)
        }
        "flowChartPunchedCard" => {
            simple(b.poly(&[(w * 0.2, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, h * 0.2)]).take(), Rect::new(0.0, h * 0.2, w, h), w, h)
        }
        "flowChartPunchedTape" => {
            // ECMA-376: each long edge is two half-ellipses a quarter of the width wide and a
            // tenth of the height deep, one bulging in and one out.
            let (rx, ry) = (w / 4.0, h / 10.0);
            b.m(0.0, ry).arc(rx, ry, 180.0, -180.0).arc(rx, ry, 180.0, 180.0).l(w, h - ry).arc(rx, ry, 0.0, -180.0).arc(rx, ry, 0.0, 180.0).z();
            simple(b.take(), Rect::new(0.0, h / 5.0, w, h * 0.8), w, h)
        }
        "flowChartSummingJunction" | "flowChartOr" => {
            let e = Pb::new().ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0).take();
            let mut l = Pb::new();
            if name == "flowChartOr" {
                l.m(w / 2.0, 0.0).l(w / 2.0, h).m(0.0, h / 2.0).l(w, h / 2.0);
            } else {
                let (dx, dy) = (w / 2.0 * std::f64::consts::FRAC_1_SQRT_2, h / 2.0 * std::f64::consts::FRAC_1_SQRT_2);
                l.m(w / 2.0 - dx, h / 2.0 - dy).l(w / 2.0 + dx, h / 2.0 + dy).m(w / 2.0 + dx, h / 2.0 - dy).l(w / 2.0 - dx, h / 2.0 + dy);
            }
            Geometry {
                paths: vec![fill(e), stroke_only(l.take())],
                text_rect: Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355),
                handles: vec![],
                sites: ellipse_sites(w, h),
            }
        }
        "flowChartCollate" => simple(b.poly(&[(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]).take(), Rect::new(w / 4.0, h / 4.0, w * 0.75, h * 0.75), w, h),
        "flowChartOnlineStorage" => {
            let rx = w / 6.0;
            b.m(rx, 0.0).l(w, 0.0).arc(rx, h / 2.0, 270.0, -180.0).l(rx, h).arc(rx, h / 2.0, 90.0, 180.0).z();
            simple(b.take(), Rect::new(rx, 0.0, w - rx, h), w, h)
        }
        "flowChartDelay" => {
            b.m(0.0, 0.0).l(w / 2.0, 0.0).arc(w / 2.0, h / 2.0, 270.0, 180.0).l(0.0, h).z();
            simple(b.take(), Rect::new(0.0, h * 0.15, w * 0.85, h * 0.85), w, h)
        }
        "flowChartMagneticTape" => {
            b.m(w / 2.0, h).arc(w / 2.0, h / 2.0, 90.0, 90.0).arc(w / 2.0, h / 2.0, 180.0, 90.0).arc(w / 2.0, h / 2.0, 270.0, 90.0).arc(
                w / 2.0,
                h / 2.0,
                0.0,
                45.0,
            );
            let p = b.cur;
            b.l(w, p.y).l(w, h).z();
            simple(b.take(), Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355), w, h)
        }
        "flowChartDisplay" => {
            let rx = w / 6.0;
            b.m(0.0, h / 2.0).l(rx, 0.0).l(w - rx, 0.0).arc(rx, h / 2.0, 270.0, 180.0).l(rx, h).z();
            simple(b.take(), Rect::new(rx, 0.0, w - rx, h), w, h)
        }
        "star4" | "star5" | "star6" | "star7" | "star8" | "star10" | "star12" | "star16" | "star24" | "star32" => {
            let n: usize = name.trim_start_matches("star").parse().unwrap_or(5);
            let v = a(0).clamp(0.0, 50000.0);
            let inner = v / 50000.0;
            let text = star_text_rect(n, w, h, inner).unwrap_or_else(|| full.inset(-ss * (0.5 - inner * 0.5) * 0.5));
            let mut g = simple(b.poly(&star(n, w, h, inner)).take(), text, w, h);
            g.handles.push(Handle::new(0, Point::new(w / 2.0, h / 2.0), Vec2::new(0.0, -h / 2.0 / 50000.0), v, 0.0, 50000.0));
            g
        }
        "irregularSeal1" => {
            // ECMA-376 irregularSeal1 ("Explosion: 8 Points"), in 21600ths of the box.
            let pts = spec_pts(
                &[
                    (10800, 5800),
                    (14522, 0),
                    (14155, 5325),
                    (18380, 4457),
                    (16702, 7315),
                    (21097, 8137),
                    (17607, 10475),
                    (21600, 13290),
                    (16837, 12942),
                    (18145, 18095),
                    (14020, 14457),
                    (13247, 19737),
                    (10532, 14935),
                    (8485, 21600),
                    (7715, 15627),
                    (4762, 17617),
                    (5667, 13937),
                    (135, 14587),
                    (3722, 11775),
                    (0, 8615),
                    (4627, 7617),
                    (370, 2295),
                    (7312, 6320),
                    (8352, 2295),
                ],
                w,
                h,
            );
            simple(b.poly(&pts).take(), Rect::new(w * 4627.0 / 21600.0, h * 6320.0 / 21600.0, w * 16702.0 / 21600.0, h * 13937.0 / 21600.0), w, h)
        }
        "irregularSeal2" => {
            // ECMA-376 irregularSeal2 ("Explosion: 14 Points"), in 21600ths of the box.
            let pts = spec_pts(
                &[
                    (11462, 4342),
                    (14790, 0),
                    (14525, 5777),
                    (18007, 3172),
                    (16380, 6532),
                    (21600, 6645),
                    (16985, 9402),
                    (18270, 11290),
                    (16380, 12310),
                    (18877, 15632),
                    (14640, 14350),
                    (14942, 17370),
                    (12180, 15935),
                    (11612, 18842),
                    (9872, 17370),
                    (8700, 19712),
                    (7527, 18125),
                    (4917, 21600),
                    (4805, 18240),
                    (1285, 17825),
                    (3330, 15370),
                    (0, 12877),
                    (3935, 11592),
                    (1172, 8270),
                    (5372, 7817),
                    (4502, 3625),
                    (8550, 6382),
                    (9722, 1887),
                ],
                w,
                h,
            );
            simple(b.poly(&pts).take(), Rect::new(w * 5372.0 / 21600.0, h * 6382.0 / 21600.0, w * 14640.0 / 21600.0, h * 15935.0 / 21600.0), w, h)
        }
        "ribbon" | "ribbon2" => ribbon(name == "ribbon2", w, h, a(0), a(1)),
        "verticalScroll" | "horizontalScroll" => scroll(name == "verticalScroll", w, h, a(0)),
        "wave" | "doubleWave" => {
            // ECMA-376 wave / doubleWave: the top and bottom edges are cubic waves (one or two
            // periods) of amplitude adj1, shifted sideways by adj2.
            let a1 = a(0).clamp(0.0, if name == "wave" { 20000.0 } else { 12500.0 });
            let a2 = a(1).clamp(-10000.0, 10000.0);
            let y1 = h * a1 / 100000.0;
            let dy2 = y1 * 10.0 / 3.0;
            let (y2, y3, y4) = (y1 - dy2, y1 + dy2, h - y1);
            let (y5, y6) = (y4 - dy2, y4 + dy2);
            let of2 = w * a2 / 50000.0;
            let dx2 = of2.min(0.0);
            let dxr = of2.max(0.0);
            let (x2, xr, xl, x10) = (-dx2, w - dxr, dxr, w + dx2);
            let text_y = h * a1 / 50000.0;
            if name == "wave" {
                let dx3 = (dx2 + xr) / 3.0;
                let (x3, x7) = (x2 + dx3, xl + dx3);
                let (x4, x8) = ((x3 + xr) / 2.0, (x7 + x10) / 2.0);
                b.m(x2, y1).c(x3, y2, x4, y3, xr, y1).l(x10, y4).c(x8, y6, x7, y5, xl, y4).z();
            } else {
                let (dx3, dx4) = ((dx2 + xr) / 6.0, (dx2 + xr) / 3.0);
                let (x3, x4, x5) = (x2 + dx3, x2 + dx4, (x2 + xr) / 2.0);
                let x6 = x5 + dx3;
                let x7 = (x6 + xr) / 2.0;
                let (x10b, x11, x12) = (xl + dx3, xl + dx4, (xl + x10) / 2.0);
                let x13 = x12 + dx3;
                let x14 = (x13 + x10) / 2.0;
                b.m(x2, y1).c(x3, y2, x4, y3, x5, y1).c(x6, y2, x7, y3, xr, y1).l(x10, y4);
                b.c(x14, y6, x13, y5, x12, y4).c(x11, y6, x10b, y5, xl, y4).z();
            }
            simple(b.take(), Rect::new(x2.max(xl), text_y, xr.min(x10), h - text_y), w, h)
        }
        "wedgeRectCallout" | "wedgeRoundRectCallout" => {
            let r = if name == "wedgeRoundRectCallout" { ss * a(2).clamp(0.0, 50000.0) / 1e5 } else { 0.0 };
            wedge_rect(w, h, a(0), a(1), r)
        }
        "wedgeEllipseCallout" => {
            let tip = Point::new(w / 2.0 + w * a(0) / 1e5, h / 2.0 + h * a(1) / 1e5);
            let ang = (tip.y - h / 2.0).atan2(tip.x - w / 2.0).to_degrees();
            let s = vis(ang + 10.0, w / 2.0, h / 2.0);
            let p0 = Point::new(w / 2.0 + w / 2.0 * s.to_radians().cos(), h / 2.0 + h / 2.0 * s.to_radians().sin());
            let e = vis(ang - 10.0, w / 2.0, h / 2.0);
            let mut sw = e - s;
            if sw <= 0.0 {
                sw += 360.0;
            }
            b.m(p0.x, p0.y).arc(w / 2.0, h / 2.0, s, sw).l(tip.x, tip.y).z();
            let mut g = simple(b.take(), Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355), w, h);
            g.handles.push(Handle::new(0, Point::new(w / 2.0, h / 2.0), Vec2::new(w / 1e5, 0.0), a(0), -1e6, 1e6));
            g.handles.push(Handle::new(1, Point::new(w / 2.0, h / 2.0), Vec2::new(0.0, h / 1e5), a(1), -1e6, 1e6));
            g.handles.retain(|_| false);
            g.handles.push(tip_handle(w, h, tip));
            g
        }
        "borderCallout1" => {
            let r = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]).take();
            let p1 = (w * a(1) / 1e5, h * a(0) / 1e5);
            let p2 = (w * a(3) / 1e5, h * a(2) / 1e5);
            let l = Pb::new().m(p1.0, p1.1).l(p2.0, p2.1).take();
            Geometry { paths: vec![fill(r), stroke_only(l)], text_rect: full, handles: vec![], sites: mid_sites(w, h) }
        }
        "line" | "straightConnector1" => {
            let l = Pb::new().m(0.0, 0.0).l(w, h).take();
            Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::ZERO, Point::new(w, h)] }
        }
        "bentConnector2" => {
            let l = Pb::new().m(0.0, 0.0).l(w, 0.0).l(w, h).take();
            Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::ZERO, Point::new(w, h)] }
        }
        "bentConnector3" | "bentConnector4" | "bentConnector5" => {
            let x = w * a(0) / 1e5;
            let l = Pb::new().m(0.0, 0.0).l(x, 0.0).l(x, h).l(w, h).take();
            let mut g = Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::ZERO, Point::new(w, h)] };
            g.handles.push(Handle::new(0, Point::new(0.0, h / 2.0), Vec2::new(w / 1e5, 0.0), a(0), -1e6, 1e6));
            g
        }
        "curvedConnector2" | "curvedConnector3" | "curvedConnector4" | "curvedConnector5" => {
            let x = w * if name == "curvedConnector2" { 100000.0 } else { a(0) } / 1e5;
            let l = Pb::new().m(0.0, 0.0).c(x / 2.0, 0.0, x, h / 4.0, x, h / 2.0).c(x, h * 0.75, (x + w) / 2.0, h, w, h).take();
            Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::ZERO, Point::new(w, h)] }
        }
        _ => return extra::build(name, w, h, &a),
    };
    Some(g)
}

fn ellipse_sites(w: f64, h: f64) -> Vec<Point> {
    (0..8)
        .map(|i| {
            let a = (-90.0 - 45.0 * i as f64).to_radians();
            Point::new(w / 2.0 + w / 2.0 * a.cos(), h / 2.0 + h / 2.0 * a.sin())
        })
        .collect()
}

fn tip_handle(w: f64, h: f64, tip: Point) -> Handle {
    // A free 2-D handle is represented as index 0 with zero direction; callers move both values.
    Handle { index: 0, pos: tip, origin: Point::new(w / 2.0, h / 2.0), dir: Vec2::new(w / 1e5, h / 1e5), min: -1e6, max: 1e6, angle: false }
}

fn wedge_rect(w: f64, h: f64, a0: f64, a1: f64, r: f64) -> Geometry {
    let tip = Point::new(w / 2.0 + w * a0 / 1e5, h / 2.0 + h * a1 / 1e5);
    let inside = tip.x > 0.0 && tip.x < w && tip.y > 0.0 && tip.y < h;
    let (dx, dy) = ((tip.x - w / 2.0) / w.max(1e-9), (tip.y - h / 2.0) / h.max(1e-9));
    let mut b = Pb::new();
    let r = r.min(w / 2.0).min(h / 2.0);
    // Walk clockwise from the top-left; insert the wedge on the edge facing the tip.
    let horiz = dx.abs() > dy.abs();
    // The wedge's base: 2..5 twelfths of the edge on the tip's side of the centre when it is
    // before it, 7..10 twelfths when after (presetShapeDefinitions' xg1/xg2, yg1/yg2).
    let (bx0, bx1) = if tip.x < w / 2.0 { (w * 2.0 / 12.0, w * 5.0 / 12.0) } else { (w * 7.0 / 12.0, w * 10.0 / 12.0) };
    let (by0, by1) = if tip.y < h / 2.0 { (h * 2.0 / 12.0, h * 5.0 / 12.0) } else { (h * 7.0 / 12.0, h * 10.0 / 12.0) };
    b.m(r, 0.0);
    if !inside && !horiz && dy < 0.0 {
        b.l(bx0, 0.0).l(tip.x, tip.y).l(bx1, 0.0);
    }
    b.l(w - r, 0.0);
    if r > 0.0 {
        b.arc(r, r, 270.0, 90.0);
    }
    if !inside && horiz && dx > 0.0 {
        b.l(w, by0).l(tip.x, tip.y).l(w, by1);
    }
    b.l(w, h - r);
    if r > 0.0 {
        b.arc(r, r, 0.0, 90.0);
    }
    if !inside && !horiz && dy > 0.0 {
        b.l(bx1, h).l(tip.x, tip.y).l(bx0, h);
    }
    b.l(r, h);
    if r > 0.0 {
        b.arc(r, r, 90.0, 90.0);
    }
    if !inside && horiz && dx < 0.0 {
        b.l(0.0, by1).l(tip.x, tip.y).l(0.0, by0);
    }
    b.l(0.0, r);
    if r > 0.0 {
        b.arc(r, r, 180.0, 90.0);
    }
    b.z();
    let mut g = simple(b.take(), Rect::new(0.0, 0.0, w, h).inset(-r * 0.29289), w, h);
    g.handles.push(tip_handle(w, h, tip));
    g
}

/// ECMA-376 cloud outline arcs (radii in 43200ths of the box, angles in 60000ths of a degree,
/// as DrawingML `arcTo` takes them), from the start point (3900, 14370).
const CLOUD_ARCS: [(f64, f64, f64, f64); 11] = [
    (6753.0, 9190.0, -11429249.0, 7426832.0),
    (5333.0, 7267.0, -8646143.0, 5396714.0),
    (4365.0, 5945.0, -8748475.0, 5983381.0),
    (4857.0, 6595.0, -7859164.0, 7034504.0),
    (5333.0, 7273.0, -4722533.0, 6541615.0),
    (6775.0, 9220.0, -2776035.0, 7816140.0),
    (5785.0, 7867.0, 37501.0, 6842000.0),
    (6752.0, 9215.0, 1347096.0, 6910353.0),
    (7720.0, 10543.0, 3974558.0, 4542661.0),
    (4360.0, 5918.0, -16496525.0, 8804134.0),
    (4345.0, 5945.0, -14809710.0, 9151131.0),
];

/// The cloud's creases: short arcs inside the outline, each from its own start point.
const CLOUD_CREASES: [(f64, f64, f64, f64, f64, f64); 11] = [
    (4693.0, 26177.0, 4345.0, 5945.0, 5204520.0, 1585770.0),
    (6928.0, 34899.0, 4360.0, 5918.0, 4416628.0, 686848.0),
    (16478.0, 39090.0, 6752.0, 9215.0, 8257449.0, 844866.0),
    (28827.0, 34751.0, 6752.0, 9215.0, 387196.0, 959901.0),
    (34129.0, 22954.0, 5785.0, 7867.0, -4217541.0, 4255042.0),
    (41798.0, 15354.0, 5333.0, 7273.0, 1819082.0, 1665090.0),
    (38324.0, 5426.0, 4857.0, 6595.0, -824660.0, 891534.0),
    (29078.0, 3952.0, 4857.0, 6595.0, -8950887.0, 1091722.0),
    (22141.0, 4720.0, 4365.0, 5945.0, -9809656.0, 1061181.0),
    (14000.0, 5192.0, 6753.0, 9190.0, -4002417.0, 739161.0),
    (4127.0, 15789.0, 6753.0, 9190.0, 9459261.0, 711490.0),
];

fn cloud(name: &str, w: f64, h: f64, a0: f64, a1: f64) -> Geometry {
    // Drawn in the path's own 43200-unit square, then stretched to the box (as DrawingML lays out
    // a path with its own w and h), so the arcs keep their shape's proportions at any aspect.
    let to_box = kurbo::Affine::scale_non_uniform(w / 43200.0, h / 43200.0);
    let deg = |v: f64| v / 60000.0;
    let mut b = Pb::new();
    b.m(3900.0, 14370.0);
    for (wr, hr, st, sw) in CLOUD_ARCS {
        b.arc_spec(wr, hr, deg(st), deg(sw));
    }
    b.z();
    let mut creases = Pb::new();
    for (x, y, wr, hr, st, sw) in CLOUD_CREASES {
        creases.m(x, y).arc_spec(wr, hr, deg(st), deg(sw));
    }
    let mut outline = b.take();
    outline.apply_affine(to_box);
    let mut paths = vec![fill(outline)];
    if name == "cloud" {
        let mut c = creases.take();
        c.apply_affine(to_box);
        paths.push(stroke_only(c));
    }
    let text_rect = Rect::new(w * 2977.0 / 21600.0, h * 3262.0 / 21600.0, w * 17087.0 / 21600.0, h * 17337.0 / 21600.0);
    let mut handles = vec![];
    if name == "cloudCallout" {
        let tip = Point::new(w / 2.0 + w * a0 / 1e5, h / 2.0 + h * a1 / 1e5);
        let c = Point::new(w / 2.0, h / 2.0);
        let mut bubbles = Pb::new();
        for (k, r) in [(0.55, 0.06), (0.75, 0.045), (0.95, 0.03)] {
            let p = c + (tip - c) * k;
            if !(p.x > w * 0.1 && p.x < w * 0.9 && p.y > h * 0.1 && p.y < h * 0.9) || k > 0.9 {
                bubbles.ellipse(p.x, p.y, w * r, h * r);
            }
        }
        paths.push(fill(bubbles.take()));
        handles.push(tip_handle(w, h, tip));
    }
    Geometry { paths, text_rect, handles, sites: mid_sites(w, h) }
}

/// The equation shapes, by ECMA-376's formulas (checked against PowerPoint's outlines at several
/// sizes and adjust values). Their bars span 73.49% of the width; the plus and multiply bars are
/// `adj1` of the shorter side thick, the others' `adj1` of the height.
fn math(name: &str, w: f64, h: f64, a0: f64, a1: f64, a2: f64) -> Geometry {
    let pin = |lo: f64, v: f64, hi: f64| v.max(lo).min(hi.max(lo));
    let ss = w.min(h);
    let (hc, vc) = (w / 2.0, h / 2.0);
    let dx1 = w * 73490.0 / 200000.0;
    let (x1, x2) = (hc - dx1, hc + dx1);
    let mut b = Pb::new();
    let text = match name {
        "mathPlus" => {
            let a = pin(0.0, a0, 73490.0);
            let dy1 = h * 73490.0 / 200000.0;
            let dx2 = ss * a / 200000.0;
            let (y1, y2, y3, y4) = (vc - dy1, vc - dx2, vc + dx2, vc + dy1);
            let (xa, xb) = (hc - dx2, hc + dx2);
            b.poly(&[(x1, y2), (xa, y2), (xa, y1), (xb, y1), (xb, y2), (x2, y2), (x2, y3), (xb, y3), (xb, y4), (xa, y4), (xa, y3), (x1, y3)]);
            Rect::new(x1, y2, x2, y3)
        }
        "mathMinus" => {
            let dy1 = h * pin(0.0, a0, 100000.0) / 200000.0;
            b.poly(&[(x1, vc - dy1), (x2, vc - dy1), (x2, vc + dy1), (x1, vc + dy1)]);
            Rect::new(x1, vc - dy1, x2, vc + dy1)
        }
        "mathEqual" => {
            let a = pin(0.0, a0, 36745.0);
            let g = pin(0.0, a1, 100000.0 - 2.0 * a);
            let (dy1, dy2) = (h * a / 100000.0, h * g / 200000.0);
            let (y2, y3) = (vc - dy2, vc + dy2);
            let (y1, y4) = (y2 - dy1, y3 + dy1);
            b.poly(&[(x1, y1), (x2, y1), (x2, y2), (x1, y2)]);
            b.poly(&[(x1, y3), (x2, y3), (x2, y4), (x1, y4)]);
            Rect::new(x1, y1, x2, y4)
        }
        "mathNotEqual" => {
            let a = pin(0.0, a0, 50000.0);
            let cr_ang = pin(4200000.0, a1, 6600000.0);
            let g = pin(0.0, a2, 100000.0 - 2.0 * a);
            let (dy1, dy2) = (h * a / 100000.0, h * g / 200000.0);
            let hd2 = (h / 2.0).max(1e-9);
            let (y2, y3) = (vc - dy2, vc + dy2);
            let (y1, y4) = (y2 - dy1, y3 + dy1);
            let cadj2 = cr_ang - 5400000.0;
            let xadj2 = hd2 * (cadj2 / 60000.0).to_radians().tan();
            let len = xadj2.hypot(hd2);
            let bhw = len * dy1 / hd2;
            let x7 = hc + xadj2 - bhw / 2.0;
            let at = |y: f64| x7 - xadj2 * y / hd2;
            let (x6, x5, x4, x3) = (at(y1), at(y2), at(y3), at(y4));
            let rx7 = x7 + bhw;
            let dx7 = dy1 * hd2 / len;
            let dy3 = dy1 * xadj2 / len;
            let (rx, lx, ry, ly) = if cadj2 > 0.0 { (x7 + dx7, x7, dy3, 0.0) } else { (rx7, rx7 - dx7, 0.0, -dy3) };
            b.poly(&[
                (x1, y1),
                (x6, y1),
                (lx, ly),
                (rx, ry),
                (x6 + bhw, y1),
                (x2, y1),
                (x2, y2),
                (x5 + bhw, y2),
                (x4 + bhw, y3),
                (x2, y3),
                (x2, y4),
                (x3 + bhw, y4),
                (w - lx, h - ly),
                (w - rx, h - ry),
                (x3, y4),
                (x1, y4),
                (x1, y3),
                (x4, y3),
                (x5, y2),
                (x1, y2),
            ]);
            Rect::new(x1, y1, x2, y4)
        }
        "mathDivide" => {
            let a = pin(1000.0, a0, 36745.0);
            let max3 = ((73490.0 - a) / 4.0).min(36745.0 * w / h.max(1e-9));
            let r3 = pin(1000.0, a2, max3);
            let g = pin(0.0, a1, 73490.0 - 4.0 * r3 - a);
            let dy1 = h * a / 200000.0;
            let yg = h * g / 100000.0;
            let rad = h * r3 / 100000.0;
            let (y3, y4) = (vc - dy1, vc + dy1);
            let y2 = y3 - (yg + rad);
            let y1 = y2 - rad;
            b.ellipse(hc, y1 + rad, rad, rad);
            b.ellipse(hc, h - y1 - rad, rad, rad);
            b.poly(&[(x1, y3), (x2, y3), (x2, y4), (x1, y4)]);
            Rect::new(x1, y3, x2, y4)
        }
        _ => {
            // mathMultiply: an X whose arms run along the box's diagonals.
            let th = ss * pin(0.0, a0, 51965.0) / 100000.0;
            let ang = h.atan2(w);
            let (sa, ca, ta) = (ang.sin(), ang.cos(), ang.tan().max(1e-9));
            let lm = w.hypot(h) * (1.0 - 51965.0 / 100000.0);
            let (xm, ym) = (ca * lm / 2.0, sa * lm / 2.0);
            let (dxam, dyam) = (sa * th / 2.0, ca * th / 2.0);
            let (xa, ya, xb, yb) = (xm - dxam, ym + dyam, xm + dxam, ym - dyam);
            let yc = (hc - xb) * ta + yb;
            let (xd, xe) = (w - xb, w - xa);
            let xfe = (vc - ya) / ta;
            let (xf, xl) = (xe - xfe, xa + xfe);
            let (yg, yh, yi) = (h - ya, h - yb, h - yc);
            b.poly(&[(xa, ya), (xb, yb), (hc, yc), (xd, yb), (xe, ya), (xf, vc), (xe, yg), (xd, yh), (hc, yi), (xb, yh), (xa, yg), (xl, vc)]);
            Rect::new(xa, yb, xe, yh)
        }
    };
    Geometry { paths: vec![fill(b.take())], text_rect: text, handles: vec![], sites: mid_sites(w, h) }
}

fn ribbon(up: bool, w: f64, h: f64, a0: f64, a1: f64) -> Geometry {
    let fold = h * a0.clamp(0.0, 33333.0) / 1e5;
    let cw = w * a1.clamp(25000.0, 75000.0) / 1e5; // centre band width
    let x0 = (w - cw) / 2.0;
    let x1 = w - x0;
    let ear = w / 8.0;
    let (top, bot) = (0.0, h);
    let mut b = Pb::new();
    let mut shade_p = Pb::new();
    if up {
        // centre band sits high; ends hang low.
        b.poly(&[(x0, top), (x1, top), (x1, bot - fold), (x0, bot - fold)]);
        let mut ends = Pb::new();
        ends.poly(&[(0.0, fold), (x0 + ear * 0.5, fold), (x0 + ear * 0.5, bot), (0.0, bot), (ear, (fold + bot) / 2.0)]);
        ends.poly(&[(w, fold), (x1 - ear * 0.5, fold), (x1 - ear * 0.5, bot), (w, bot), (w - ear, (fold + bot) / 2.0)]);
        shade_p.poly(&[(x0, bot - fold), (x0 + ear * 0.5, bot - fold), (x0 + ear * 0.5, bot)]);
        shade_p.poly(&[(x1, bot - fold), (x1 - ear * 0.5, bot - fold), (x1 - ear * 0.5, bot)]);
        let text = Rect::new(x0, top, x1, bot - fold);
        return Geometry {
            paths: vec![fill(ends.take()), shade(shade_p.take(), FillMode::DarkenLess), fill(b.take())],
            text_rect: text,
            handles: vec![],
            sites: mid_sites(w, h),
        };
    }
    b.poly(&[(x0, top + fold), (x1, top + fold), (x1, bot), (x0, bot)]);
    let mut ends = Pb::new();
    ends.poly(&[(0.0, top), (x0 + ear * 0.5, top), (x0 + ear * 0.5, bot - fold), (0.0, bot - fold), (ear, (bot - fold) / 2.0)]);
    ends.poly(&[(w, top), (x1 - ear * 0.5, top), (x1 - ear * 0.5, bot - fold), (w, bot - fold), (w - ear, (bot - fold) / 2.0)]);
    shade_p.poly(&[(x0, top + fold), (x0 + ear * 0.5, top + fold), (x0 + ear * 0.5, top)]);
    shade_p.poly(&[(x1, top + fold), (x1 - ear * 0.5, top + fold), (x1 - ear * 0.5, top)]);
    let text = Rect::new(x0, top + fold, x1, bot);
    Geometry {
        paths: vec![fill(ends.take()), shade(shade_p.take(), FillMode::DarkenLess), fill(b.take())],
        text_rect: text,
        handles: vec![],
        sites: mid_sites(w, h),
    }
}

fn scroll(vertical: bool, w: f64, h: f64, a0: f64) -> Geometry {
    let ss = w.min(h);
    let r = ss * a0.clamp(0.0, 25000.0) / 1e5 / 2.0;
    let mut b = Pb::new();
    let mut curl = Pb::new();
    if vertical {
        b.poly(&[(r, r * 2.0), (w - r, r * 2.0), (w - r, h - r), (r, h - r)]);
        b.ellipse(w / 2.0, r, w / 2.0 - r * 0.0, r);
        curl.ellipse(r, h - r, r, r);
        curl.ellipse(w - r * 2.0, r, r * 0.5, r * 0.5);
        let text = Rect::new(r, r * 2.0, w - r, h - r);
        return Geometry {
            paths: vec![fill(b.take()), shade(curl.take(), FillMode::DarkenLess)],
            text_rect: text,
            handles: vec![],
            sites: mid_sites(w, h),
        };
    }
    b.poly(&[(r * 2.0, r), (w - r, r), (w - r, h - r), (r * 2.0, h - r)]);
    b.ellipse(r, h / 2.0, r, h / 2.0);
    curl.ellipse(w - r, r, r, r);
    curl.ellipse(r, h - r * 2.0, r * 0.5, r * 0.5);
    let text = Rect::new(r * 2.0, r, w - r, h - r);
    Geometry { paths: vec![fill(b.take()), shade(curl.take(), FillMode::DarkenLess)], text_rect: text, handles: vec![], sites: mid_sites(w, h) }
}

fn action_button(name: &str, w: f64, h: f64) -> Geometry {
    let body = corner_rect(w, h, [Corner::Square; 4]);
    let ss = w.min(h);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let s = ss * 0.3;
    let mut g = Pb::new();
    match name {
        "actionButtonBackPrevious" => {
            g.poly(&[(cx + s, cy - s), (cx + s, cy + s), (cx - s, cy)]);
        }
        "actionButtonForwardNext" => {
            g.poly(&[(cx - s, cy - s), (cx - s, cy + s), (cx + s, cy)]);
        }
        "actionButtonBeginning" => {
            g.poly(&[(cx + s, cy - s), (cx + s, cy + s), (cx - s * 0.6, cy)]);
            g.poly(&[(cx - s, cy - s), (cx - s * 0.7, cy - s), (cx - s * 0.7, cy + s), (cx - s, cy + s)]);
        }
        "actionButtonEnd" => {
            g.poly(&[(cx - s, cy - s), (cx - s, cy + s), (cx + s * 0.6, cy)]);
            g.poly(&[(cx + s * 0.7, cy - s), (cx + s, cy - s), (cx + s, cy + s), (cx + s * 0.7, cy + s)]);
        }
        "actionButtonHome" => {
            g.poly(&[
                (cx, cy - s),
                (cx + s, cy),
                (cx + s * 0.7, cy),
                (cx + s * 0.7, cy + s),
                (cx - s * 0.7, cy + s),
                (cx - s * 0.7, cy),
                (cx - s, cy),
            ]);
        }
        "actionButtonInformation" | "actionButtonHelp" => {
            g.ellipse(cx, cy, s, s);
        }
        "actionButtonReturn" => {
            g.m(cx + s, cy - s * 0.6).l(cx + s, cy + s * 0.2).q(cx + s, cy + s * 0.8, cx + s * 0.4, cy + s * 0.8).l(cx - s * 0.5, cy + s * 0.8);
            g.l(cx - s * 0.5, cy + s * 0.4)
                .l(cx - s, cy + s * 0.9)
                .l(cx - s * 0.5, cy + s * 1.4)
                .l(cx - s * 0.5, cy + s)
                .l(cx + s * 0.6, cy + s)
                .l(cx + s * 0.6, cy - s * 0.6)
                .z();
        }
        "actionButtonMovie" => {
            g.poly(&[(cx - s, cy - s * 0.6), (cx + s * 0.4, cy - s * 0.6), (cx + s * 0.4, cy + s * 0.6), (cx - s, cy + s * 0.6)]);
            g.poly(&[(cx + s * 0.4, cy), (cx + s, cy - s * 0.5), (cx + s, cy + s * 0.5)]);
        }
        "actionButtonDocument" => {
            g.poly(&[(cx - s * 0.7, cy - s), (cx + s * 0.3, cy - s), (cx + s * 0.7, cy - s * 0.6), (cx + s * 0.7, cy + s), (cx - s * 0.7, cy + s)]);
        }
        "actionButtonSound" => {
            g.poly(&[
                (cx - s, cy - s * 0.35),
                (cx - s * 0.5, cy - s * 0.35),
                (cx, cy - s),
                (cx, cy + s),
                (cx - s * 0.5, cy + s * 0.35),
                (cx - s, cy + s * 0.35),
            ]);
        }
        _ => {}
    }
    Geometry {
        paths: vec![fill(body), shade(g.take(), FillMode::DarkenLess)],
        text_rect: Rect::new(0.0, 0.0, w, h),
        handles: vec![],
        sites: mid_sites(w, h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    #[test]
    fn every_catalog_preset_builds_within_bounds() {
        for p in CATALOG {
            for (w, h) in [(200.0, 100.0), (100.0, 200.0), (50.0, 50.0), (0.0, 0.0), (1.0, 300.0)] {
                let g = build(p.name, w, h, &[]).unwrap_or_else(|| panic!("{} did not build", p.name));
                assert!(!g.paths.is_empty(), "{} has no paths", p.name);
                for s in &g.paths {
                    let bb = s.path.bounding_box();
                    if w > 0.0 && h > 0.0 && !p.name.contains("Connector") && !p.name.contains("allout") {
                        let tol = w.max(h) * 0.35 + 1.0;
                        assert!(bb.x0 >= -tol && bb.y0 >= -tol && bb.x1 <= w + tol && bb.y1 <= h + tol, "{} {w}x{h} out of bounds: {bb:?}", p.name);
                    }
                    assert!(bb.x0.is_finite() && bb.y1.is_finite(), "{} produced non-finite geometry", p.name);
                }
                assert!(g.text_rect.x0.is_finite() && g.text_rect.y1.is_finite(), "{}", p.name);
            }
        }
    }

    #[test]
    fn hostile_adjust_values_never_panic() {
        for p in CATALOG {
            for v in [f64::NAN, f64::INFINITY, -1e12, 1e12, -1.0, 0.0] {
                let adj = vec![v; 6];
                let g = build(p.name, 120.0, 80.0, &adj);
                assert!(g.is_some(), "{}", p.name);
            }
        }
        assert!(build("rect", f64::NAN, 10.0, &[]).is_none());
        assert!(build("noSuchShape", 10.0, 10.0, &[]).is_none());
    }

    #[test]
    fn catalog_names_are_unique() {
        let mut names: Vec<&str> = CATALOG.iter().map(|p| p.name).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len());
        assert!(n >= 140, "catalog has {n} presets");
    }

    #[test]
    fn round_rect_handle_maps_back() {
        let g = build("roundRect", 200.0, 100.0, &[16667.0]).unwrap();
        let h = g.handles.first().unwrap();
        assert!((h.value_at(h.pos) - 16667.0).abs() <= 1.0);
        assert_eq!(h.value_at(Point::new(-50.0, 0.0)), 0.0);
        assert_eq!(h.value_at(Point::new(500.0, 0.0)), 50000.0);
    }

    #[test]
    fn partial_circle_angle_handles_map_back() {
        // Start 0° sits at the right of the ellipse, end 270° at its top.
        for name in ["pie", "chord", "arc"] {
            let g = build(name, 200.0, 100.0, &[0.0, 16200000.0]).unwrap();
            assert_eq!(g.handles.iter().map(|h| h.index).collect::<Vec<_>>(), vec![0, 1], "{name}");
            let (s, e) = (&g.handles[0], &g.handles[1]);
            assert!((s.pos - Point::new(200.0, 50.0)).hypot() < 1e-6, "{name} {:?}", s.pos);
            assert!((e.pos - Point::new(100.0, 0.0)).hypot() < 1e-6, "{name} {:?}", e.pos);
            assert_eq!(s.value_at(s.pos), 0.0, "{name}");
            assert_eq!(e.value_at(e.pos), 16200000.0, "{name}");
            // Dragging to the bottom of the ellipse gives 90°.
            assert_eq!(s.value_at(Point::new(100.0, 100.0)), 5400000.0, "{name}");
            // Off-axis angles on a non-square ellipse: the visual 45° ray meets the 200×100
            // ellipse at offset (√2000, √2000) from the centre.
            let k = 2000f64.sqrt();
            let g = build(name, 200.0, 100.0, &[2700000.0, 8100000.0]).unwrap();
            let (s, e) = (&g.handles[0], &g.handles[1]);
            assert!((s.pos - Point::new(100.0 + k, 50.0 + k)).hypot() < 1e-6, "{name} {:?}", s.pos);
            assert!((e.pos - Point::new(100.0 - k, 50.0 + k)).hypot() < 1e-6, "{name} {:?}", e.pos);
            for (deg, h) in [(45.0, s), (135.0, e)] {
                let on = ((h.pos.x - 100.0) / 100.0).powi(2) + ((h.pos.y - 50.0) / 50.0).powi(2);
                assert!((on - 1.0).abs() < 1e-9, "{name} {deg}° off ellipse: {:?}", h.pos);
                assert!((h.value_at(h.pos) - deg * 60000.0).abs() <= 1.0, "{name} {deg}°: {}", h.value_at(h.pos));
            }
        }
        for deg in [45.0, 135.0, 200.0, 315.0] {
            let g = build("blockArc", 200.0, 100.0, &[deg * 60000.0, 0.0, 25000.0]).unwrap();
            let start = g.handles.iter().find(|h| h.index == 0).unwrap();
            // Thickness 25 → the handle runs on the 87.5×37.5 mid-ring ellipse.
            let on = ((start.pos.x - 100.0) / 87.5).powi(2) + ((start.pos.y - 50.0) / 37.5).powi(2);
            assert!((on - 1.0).abs() < 1e-9, "blockArc {deg}° off ring: {:?}", start.pos);
            let d = start.pos - Point::new(100.0, 50.0);
            assert!((d.y.atan2(d.x).to_degrees().rem_euclid(360.0) - deg).abs() < 1e-6, "blockArc {deg}°: {:?}", start.pos);
            assert!((start.value_at(start.pos) - deg * 60000.0).abs() <= 1.0, "blockArc {deg}°");
        }
        let g = build("blockArc", 200.0, 100.0, &[10800000.0, 0.0, 25000.0]).unwrap();
        let start = g.handles.iter().find(|h| h.index == 0).unwrap();
        assert_eq!(start.value_at(start.pos), 10800000.0);
        let thick = g.handles.iter().find(|h| h.index == 2).unwrap();
        assert!((thick.value_at(thick.pos) - 25000.0).abs() <= 1.0);
    }

    #[test]
    fn presets_with_adjust_values_expose_handles() {
        // Presets whose adjust values have no handle of their own yet.
        const NO_HANDLES: &[&str] = &[
            "curvedConnector3",
            "snip2SameRect",
            "snip2DiagRect",
            "snipRoundRect",
            "round1Rect",
            "round2SameRect",
            "round2DiagRect",
            "teardrop",
            "halfFrame",
            "corner",
            "diagStripe",
            "foldedCorner",
            "smileyFace",
            "moon",
            "gear6",
            "gear9",
            "bracketPair",
            "bracePair",
            "leftBracket",
            "rightBracket",
            "leftBrace",
            "rightBrace",
            "leftRightArrow",
            "upDownArrow",
            "quadArrow",
            "leftRightUpArrow",
            "bentArrow",
            "uturnArrow",
            "curvedRightArrow",
            "curvedLeftArrow",
            "curvedUpArrow",
            "curvedDownArrow",
            "leftUpArrow",
            "bentUpArrow",
            "circularArrow",
            "leftCircularArrow",
            "leftRightCircularArrow",
            "swooshArrow",
            "rightArrowCallout",
            "downArrowCallout",
            "leftArrowCallout",
            "upArrowCallout",
            "leftRightArrowCallout",
            "upDownArrowCallout",
            "quadArrowCallout",
            "mathPlus",
            "mathMinus",
            "mathMultiply",
            "mathDivide",
            "mathEqual",
            "mathNotEqual",
            "ribbon2",
            "ribbon",
            "ellipseRibbon2",
            "ellipseRibbon",
            "leftRightRibbon",
            "verticalScroll",
            "horizontalScroll",
            "wave",
            "doubleWave",
            "borderCallout1",
            "borderCallout2",
            "borderCallout3",
            "accentCallout1",
            "accentCallout2",
            "accentCallout3",
            "callout1",
            "callout2",
            "callout3",
            "accentBorderCallout1",
            "accentBorderCallout2",
            "accentBorderCallout3",
        ];
        for p in CATALOG {
            if p.defaults.is_empty() || NO_HANDLES.contains(&p.name) {
                continue;
            }
            let g = build(p.name, 200.0, 100.0, &[]).unwrap();
            assert!(!g.handles.is_empty(), "{} has adjust values but no handles", p.name);
        }
    }

    #[test]
    fn rect_area_and_ellipse_area() {
        let g = build("rect", 100.0, 50.0, &[]).unwrap();
        assert!((g.paths[0].path.area().abs() - 5000.0).abs() < 1e-6);
        let e = build("ellipse", 100.0, 50.0, &[]).unwrap();
        let expect = std::f64::consts::PI * 50.0 * 25.0;
        assert!((e.paths[0].path.area().abs() - expect).abs() / expect < 0.01);
    }

    #[test]
    fn lines_are_open() {
        assert!(build("line", 10.0, 10.0, &[]).unwrap().is_open());
        assert!(!build("rect", 10.0, 10.0, &[]).unwrap().is_open());
    }

    fn on_path(g: &Geometry, p: Point, tol: f64) -> bool {
        use kurbo::ParamCurveNearest;
        g.paths.iter().any(|sp| sp.path.segments().any(|seg| seg.nearest(p, 1e-6).distance_sq <= tol * tol))
    }

    #[test]
    fn fixed_shapes_follow_the_standards_coordinates() {
        // Points of ECMA-376's definitions (in 21600ths), checked on a 216 × 108 box.
        let (w, h) = (216.0, 108.0);
        let at = |x: f64, y: f64| Point::new(w * x / 21600.0, h * y / 21600.0);
        let g = build("lightningBolt", w, h, &[]).unwrap();
        for (x, y) in [(8472.0, 0.0), (16577.0, 12007.0), (21600.0, 21600.0), (0.0, 3890.0)] {
            assert!(on_path(&g, at(x, y), 1e-6), "lightningBolt ({x}, {y})");
        }
        let g = build("irregularSeal1", w, h, &[]).unwrap();
        assert!(on_path(&g, at(14522.0, 0.0), 1e-6) && on_path(&g, at(8485.0, 21600.0), 1e-6));
        let g = build("irregularSeal2", w, h, &[]).unwrap();
        assert!(on_path(&g, at(14790.0, 0.0), 1e-6) && on_path(&g, at(4917.0, 21600.0), 1e-6));
        // The heart's point is the middle of the bottom edge, its notch a quarter down.
        let g = build("heart", w, h, &[]).unwrap();
        assert!(on_path(&g, Point::new(w / 2.0, h), 1e-6) && on_path(&g, Point::new(w / 2.0, h / 4.0), 1e-6));
        let g = build("flowChartDocument", w, h, &[]).unwrap();
        assert!(on_path(&g, at(21600.0, 17322.0), 1e-6) && on_path(&g, at(0.0, 20172.0), 1e-6));
        // Punched tape: the top edge dips to a fifth of the height a quarter of the way along.
        let g = build("flowChartPunchedTape", w, h, &[]).unwrap();
        assert!(on_path(&g, Point::new(w / 4.0, h / 5.0), 1e-3) && on_path(&g, Point::new(w * 0.75, 0.0), 1e-3));
        let g = build("flowChartMultidocument", w, h, &[]).unwrap();
        assert!(on_path(&g, at(2972.0, 0.0), 1e-6) && on_path(&g, at(18595.0, 18022.0), 1e-6));
    }

    #[test]
    fn notched_arrow_notch_parallels_the_head() {
        // presetShapeDefinitions: the notch's point is at x1 = dy1 · dx2 / hd2, so its edges run
        // parallel to the head's (25 on a 200 × 100 arrow at the default adjusts).
        for (w, h, adj, x1) in [(200.0, 100.0, [50000.0, 50000.0], 25.0), (300.0, 120.0, [80000.0, 30000.0], 28.8)] {
            let g = build("notchedRightArrow", w, h, &adj).unwrap();
            let notch = g.paths[0].path.elements().iter().find_map(|e| match e {
                kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) if (p.y - h / 2.0).abs() < 1e-9 && p.x < w / 2.0 => Some(*p),
                _ => None,
            });
            assert!(notch.is_some_and(|p| (p.x - x1).abs() < 1e-6), "{w}x{h} {adj:?}: notch at {notch:?}, want x = {x1}");
        }
    }

    #[test]
    fn equation_shapes_match_powerpoints_outlines() {
        // Corners read off PowerPoint's own outlines of the presets at their default adjusts.
        let g = build("mathMultiply", 216.0, 216.0, &[]).unwrap();
        for (x, y) in [(0.32333, 0.15702), (0.5, 0.33369), (0.84298, 0.32333), (0.66631, 0.5), (0.33369, 0.5)] {
            assert!(on_path(&g, Point::new(216.0 * x, 216.0 * y), 0.01), "multiply ({x}, {y})");
        }
        let g = build("mathEqual", 300.0, 120.0, &[]).unwrap();
        for (x, y) in [(0.13255, 0.5588), (0.86745, 0.794), (0.13255, 0.206), (0.86745, 0.4412)] {
            assert!(on_path(&g, Point::new(300.0 * x, 120.0 * y), 0.01), "equal ({x}, {y})");
        }
        let g = build("mathPlus", 216.0, 216.0, &[]).unwrap();
        assert!(on_path(&g, Point::new(216.0 * 0.3824, 216.0 * 0.13255), 0.01) && on_path(&g, Point::new(216.0 * 0.86745, 216.0 * 0.6176), 0.01));
        // A tall "not equal": its slash, leaning at the default 110° from top to bottom, reaches
        // past the box's sides exactly as far as PowerPoint's (-22.242 to 122.242).
        let g = build("mathNotEqual", 100.0, 260.0, &[]).unwrap();
        use kurbo::Shape as _;
        let bb = g.paths[0].path.bounding_box();
        assert!((bb.x0 + 22.242).abs() < 0.01 && (bb.x1 - 122.242).abs() < 0.01 && bb.y0.abs() < 1e-6 && (bb.y1 - 260.0).abs() < 1e-6, "{bb:?}");
    }

    #[test]
    fn sun_and_waves_match_powerpoints_outlines() {
        // Points read off PowerPoint's outlines of the presets (216 pt square, default adjusts).
        let k = 216.0;
        let g = build("sun", k, k, &[]).unwrap();
        for (x, y) in [(0.85352, 0.14644), (0.76161, 0.33987), (0.66013, 0.23839), (0.42825, 0.79823), (0.20177, 0.57175), (0.75, 0.5), (0.5, 0.25)] {
            assert!(on_path(&g, Point::new(k * x, k * y), 0.02), "sun ({x}, {y})");
        }
        let g = build("wave", k, k, &[]).unwrap();
        for (x, y) in [(0.21875, 0.00484), (1.0, 0.125), (1.0, 0.875), (0.0, 0.875)] {
            assert!(on_path(&g, Point::new(k * x, k * y), 0.05), "wave ({x}, {y})");
        }
        // Shifted: a wave of 300 × 120 with adj (20000, 8000) starts 0.16 of the width in.
        let g = build("wave", 300.0, 120.0, &[20000.0, 8000.0]).unwrap();
        for (x, y) in [(0.18375, 0.00774), (0.84, 0.2), (1.0, 0.8), (0.16, 0.8)] {
            assert!(on_path(&g, Point::new(300.0 * x, 120.0 * y), 0.05), "shifted wave ({x}, {y})");
        }
    }

    #[test]
    fn star_text_rectangles_follow_the_standard() {
        // star5 at its default 19098 in a 144 pt square: the inner pentagon, about 0.38 of the
        // width (PowerPoint wraps "Shape 3" at 18 pt there), from y ≈ 55 to ≈ 110.
        let g = build("star5", 144.0, 144.0, &[]).unwrap();
        let t = g.text_rect;
        assert!((t.width() - 55.0).abs() < 1.0, "{t:?}");
        assert!((t.y0 - 55.0).abs() < 1.0 && (t.y1 - 110.0).abs() < 1.0, "{t:?}");
        // star4 at 12500: the square through its inner points' 45° corners.
        let g = build("star4", 144.0, 144.0, &[]).unwrap();
        assert!((g.text_rect.width() - 72.0 * 0.25 * 2f64.sqrt()).abs() < 0.5, "{:?}", g.text_rect);
    }

    #[test]
    fn stars_are_stretched_like_the_standards() {
        // star5 in a 216 pt square: top point at the top middle, side points at 18 degrees on an
        // ellipse 5.146% wider and 10.557% taller, centred 10.557% below the middle; its bottom
        // points reach the box's bottom edge.
        let k = 216.0;
        let g = build("star5", k, k, &[]).unwrap();
        let (rx, ry, cy) = (k / 2.0 * 1.05146, k / 2.0 * 1.10557, k / 2.0 * 1.10557);
        let a = 18f64.to_radians();
        assert!(on_path(&g, Point::new(k / 2.0, 0.0), 1e-6));
        assert!(on_path(&g, Point::new(k / 2.0 + rx * a.cos(), cy - ry * a.sin()), 1e-6));
        use kurbo::Shape as _;
        let bb = g.paths[0].path.bounding_box();
        assert!((bb.x0).abs() < 0.5 && (bb.x1 - k).abs() < 0.5 && (bb.y1 - k).abs() < 0.5, "{bb:?}");
        // star6 is 15.47% wider than its box's half-width allows (its points touch the sides).
        let g = build("star6", k, k, &[]).unwrap();
        let bb = g.paths[0].path.bounding_box();
        assert!((bb.x0).abs() < 0.5 && (bb.x1 - k).abs() < 0.5, "{bb:?}");
    }

    #[test]
    fn moon_disk_and_drum_match_powerpoints_outlines() {
        let k = 216.0;
        // Moon, default: the inner edge runs through (0.5, 0.5) and (0.625, 0.2266) of the box.
        let g = build("moon", k, k, &[]).unwrap();
        for (x, y) in [(0.5, 0.5), (0.626, 0.2266), (0.0, 0.5), (1.0, 0.0)] {
            assert!(on_path(&g, Point::new(k * x, k * y), 0.5), "moon ({x}, {y})");
        }
        // 300 × 120 at 30%: its inner edge reaches 30% of the width in.
        let g = build("moon", 300.0, 120.0, &[30000.0]).unwrap();
        assert!(on_path(&g, Point::new(90.0, 60.0), 0.5));
        // Magnetic disk ends are ellipses h/6 high, drum ends w/6 across.
        let g = build("flowChartMagneticDisk", k, k, &[]).unwrap();
        assert!(on_path(&g, Point::new(k, k / 6.0), 0.01) && on_path(&g, Point::new(k / 2.0, k / 3.0), 0.5));
        let g = build("flowChartMagneticDrum", k, k, &[]).unwrap();
        assert!(on_path(&g, Point::new(k / 6.0, 0.0), 0.01) && on_path(&g, Point::new(0.0, k / 2.0), 0.01));
    }

    #[test]
    fn the_cloud_fills_its_box_at_any_aspect_and_has_creases() {
        use kurbo::Shape as _;
        for (w, h) in [(200.0, 200.0), (300.0, 100.0), (60.0, 180.0)] {
            let g = build("cloud", w, h, &[]).unwrap();
            let bb = g.paths[0].path.bounding_box();
            assert!(bb.x0.abs() < w * 0.02 && (bb.x1 - w).abs() < w * 0.02, "{w}×{h}: {bb:?}");
            assert!(bb.y0.abs() < h * 0.02 && (bb.y1 - h).abs() < h * 0.02, "{w}×{h}: {bb:?}");
            assert_eq!(g.paths.len(), 2, "outline and creases");
            assert!(g.paths[1].stroke && g.paths[1].fill == FillMode::None);
        }
    }

    #[test]
    fn spec_arcs_take_angles_as_directions_from_the_centre() {
        // A DrawingML arc of a 2:1 ellipse from 45° to 90°: 45° is the direction (1, 1) from the
        // centre, which on that ellipse is the point (r/√5)·(2, 1)… scaled so it lies on it.
        let mut b = Pb::new();
        b.m(0.0, 0.0).arc_spec(200.0, 100.0, 45.0, 45.0);
        let p = b.take();
        let end = p.elements().last().and_then(|e| match e {
            kurbo::PathEl::CurveTo(_, _, q) => Some(*q),
            _ => None,
        });
        // Centre: the start sits at direction 45°, i.e. at (k, k) from it with (k/200)² + (k/100)² = 1.
        let k = 1.0 / (1.0f64 / 40000.0 + 1.0 / 10000.0).sqrt();
        let c = Point::new(-k, -k);
        let q = end.unwrap();
        assert!((q.x - c.x).abs() < 1e-6 && (q.y - (c.y + 100.0)).abs() < 1e-6, "{q:?} vs centre {c:?}");
    }

    #[test]
    fn brackets_and_braces_fill_out_to_their_straight_side() {
        // PowerPoint fills a bracket or brace (when it has a fill) between its curve and the
        // straight side it opens to; only the curve is outlined.
        for name in ["leftBracket", "rightBracket", "leftBrace", "rightBrace"] {
            let g = build(name, 40.0, 100.0, &[]).unwrap();
            let filled: Vec<_> = g.paths.iter().filter(|p| p.fill != FillMode::None).collect();
            assert_eq!(filled.len(), 1, "{name}");
            assert!(!filled[0].stroke, "{name}: the filled area is not outlined");
            let area = filled[0].path.area().abs();
            assert!(area > 40.0 * 100.0 * 0.4 && area < 40.0 * 100.0, "{name}: {area}");
            assert!(g.paths.iter().any(|p| p.stroke && p.fill == FillMode::None), "{name}: the curve is outlined");
        }
    }

    /// The end points of a path's move/line elements, in order.
    fn vertices(p: &BezPath) -> Vec<Point> {
        p.elements()
            .iter()
            .filter_map(|e| match e {
                kurbo::PathEl::MoveTo(q) | kurbo::PathEl::LineTo(q) => Some(*q),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn wedge_callout_mirrors_with_its_tip() {
        // A tip below and right of the centre draws the mirror image of the same tip on the left;
        // the same for a tip right of the centre, above or below it. presetShapeDefinitions puts
        // the wedge's base at 2..5 twelfths of the edge on the near side of the centre and at
        // 7..10 twelfths on the far side.
        let (w, h) = (240.0, 120.0);
        // Sorted and deduplicated: the outline closes on the corner it starts from, which the
        // mirror moves to the opposite corner.
        let sorted = |mut v: Vec<Point>| {
            v.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
            v.dedup_by(|a, b| (*a - *b).hypot() < 1e-9);
            v
        };
        for (right, left, flip_x) in [([10000.0, 80000.0], [-10000.0, 80000.0], true), ([80000.0, 10000.0], [80000.0, -10000.0], false)] {
            let a = vertices(&build("wedgeRectCallout", w, h, &right).unwrap().paths[0].path);
            let b = vertices(&build("wedgeRectCallout", w, h, &left).unwrap().paths[0].path);
            let b: Vec<Point> = b.iter().map(|p| if flip_x { Point::new(w - p.x, p.y) } else { Point::new(p.x, h - p.y) }).collect();
            let (a, b) = (sorted(a), sorted(b));
            assert_eq!(a.len(), b.len(), "{right:?}: {a:?} vs mirrored {left:?}: {b:?}");
            for (p, q) in a.iter().zip(&b) {
                assert!((*p - *q).hypot() < 1e-6, "{right:?}: {a:?}\n is not the mirror of {left:?}: {b:?}");
            }
        }
        // The bottom wedge of a tip right of the centre leaves the edge at 7/12 and 10/12 of the width.
        let a = vertices(&build("wedgeRectCallout", w, h, &[10000.0, 80000.0]).unwrap().paths[0].path);
        for x in [w * 7.0 / 12.0, w * 10.0 / 12.0] {
            assert!(a.iter().any(|p| (p.x - x).abs() < 1e-6 && (p.y - h).abs() < 1e-6), "no wedge corner at ({x}, {h}): {a:?}");
        }
    }
}
