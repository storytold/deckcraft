use super::builder::vis;
use super::{BezPath, Deserialize, Point, Rect, Serialize, Vec2};

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
    pub(super) fn new(index: usize, origin: Point, dir: Vec2, value: f64, min: f64, max: f64) -> Self {
        Handle { index, pos: origin + dir * value, origin, dir, min, max, angle: false }
    }
    /// An angle handle on the `rx`×`ry` ellipse around `center` at visual angle `value`.
    pub(super) fn angle(index: usize, center: Point, rx: f64, ry: f64, value: f64) -> Self {
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
