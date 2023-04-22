use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, File},
    path::{Path, PathBuf},
};

use argh::FromArgs;
use resvg::{
    tiny_skia,
    usvg::{
        self, utils::view_box_to_transform, Group, Node, NodeKind, Size, Transform, Tree, ViewBox,
    },
    usvg_text_layout::{
        fontdb::{self, Database},
        TreeTextToPath,
    },
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SpriteEntity {
    height: i64,
    width: i64,
    pixel_ratio: i64,
    x: i64,
    y: i64,
}

struct CombinedEntity {
    node: Node,
    width: f64,
    height: f64,
    view_box: ViewBox,
}

struct Combined {
    width: i32,
    height: i32,
    scale: f64,
    font: Database,
    entities: HashMap<String, CombinedEntity>,
}

impl Combined {
    pub fn new(scale: f64, width: i32, height: i32) -> Self {
        let mut fontdb = fontdb::Database::new();
        fontdb.load_system_fonts();

        Self {
            width,
            height,
            scale,
            entities: Default::default(),
            font: fontdb,
        }
    }

    pub fn push_node(
        &mut self,
        name: String,
        node: Node,
        width: f64,
        height: f64,
        view_box: ViewBox,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.entities.contains_key(&name) {
            return Err(format!("node with name `{}` already exists", name).into());
        }

        self.entities.insert(
            name,
            CombinedEntity {
                node,
                width,
                height,
                view_box,
            },
        );

        Ok(())
    }

    pub fn push(&mut self, name: String, mut tree: Tree) -> Result<(), Box<dyn std::error::Error>> {
        tree.convert_text(&self.font);
        self.push_node(
            name,
            tree.root,
            tree.size.width(),
            tree.size.height(),
            tree.view_box,
        )
    }

    pub fn from_svg_files(&mut self, input: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let opt = usvg::Options::default();

        let paths = fs::read_dir(input)?;

        for res in paths {
            let entry = res?;
            let path = entry.path();

            if path.extension() == Some(OsStr::new("svg")) {
                println!("\t{}", path.display());

                let full_name = path
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .strip_suffix(".svg")
                    .unwrap();

                let svg_data = fs::read(&path)?;
                let tree = usvg::Tree::from_data(&svg_data, &opt)?;
                self.push(full_name.to_string(), tree)?;
            }
        }

        Ok(())
    }

    pub fn into_packed(self) -> Result<Packed, Box<dyn std::error::Error>> {
        use binpack2d::{bin_new, BinType, Dimension};

        let items_to_place = self
            .entities
            .values()
            .enumerate()
            .map(|(id, v)| {
                Dimension::with_id(
                    id.try_into().unwrap(),
                    ((self.scale * v.width).floor() as i64).try_into().unwrap(),
                    ((self.scale * v.height).floor() as i64).try_into().unwrap(),
                    0,
                )
            })
            .collect::<Vec<_>>();

        let mut bin = bin_new(BinType::Guillotine, self.width, self.height);
        let (_, rejected) = bin.insert_list(&items_to_place);
        bin.shrink(false);

        if !rejected.is_empty() {
            return Err("not all sprites fit into the provided width and height".into());
        }

        let root = Node::new(NodeKind::Group(Group {
            ..Default::default()
        }));

        let mut entities = HashMap::new();

        for (idx, (name, entity)) in self.entities.into_iter().enumerate() {
            let id = idx.try_into().unwrap();
            let node = entity.node;
            match bin.find_by_id(id) {
                Some(rect) => {
                    let group_viewbox = Node::new(NodeKind::Group(Group {
                        transform: {
                            view_box_to_transform(
                                entity.view_box.rect,
                                entity.view_box.aspect,
                                Size::new(entity.width, entity.height).unwrap(),
                            )
                        },
                        ..Default::default()
                    }));

                    let group_position = Node::new(NodeKind::Group(Group {
                        transform: Transform::new_translate(rect.x().into(), rect.y().into()),
                        ..Default::default()
                    }));

                    let group_scale = Node::new(NodeKind::Group(Group {
                        transform: Transform::new_scale(self.scale, self.scale),
                        ..Default::default()
                    }));

                    group_viewbox.append(node.clone());
                    group_scale.append(group_viewbox);
                    group_position.append(group_scale);
                    root.append(group_position);

                    entities.insert(
                        name,
                        SpriteEntity {
                            height: rect.height().into(),
                            width: rect.width().into(),
                            pixel_ratio: 1,
                            x: rect.x().into(),
                            y: rect.y().into(),
                        },
                    );
                }
                None => unreachable!(),
            }
        }

        let size = Size::new(bin.width().into(), bin.height().into()).unwrap();
        let tree = Tree {
            root,
            size,
            view_box: usvg::ViewBox {
                rect: size.to_rect(0.0, 0.0),
                aspect: usvg::AspectRatio::default(),
            },
        };

        Ok(Packed { tree, entities })
    }
}

pub struct Packed {
    tree: Tree,
    entities: HashMap<String, SpriteEntity>,
}

impl Packed {
    pub fn to_png(&self, output: &Path, suffix: &str) -> Result<(), Box<dyn std::error::Error>> {
        let filename = format!("sprite{}.png", suffix);
        let output_png = output.join(filename);
        let pixmap_size = self.tree.size.to_screen_size();
        let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
        if resvg::render(
            &self.tree,
            usvg::FitTo::Original,
            tiny_skia::Transform::default(),
            pixmap.as_mut(),
        )
        .is_none()
        {
            return Err(format!("renderer failed").into());
        }
        pixmap.save_png(output_png)?;

        Ok(())
    }

    pub fn to_json(&self, output: &Path, suffix: &str) -> Result<(), Box<dyn std::error::Error>> {
        let filename = format!("sprite{}.json", suffix);
        let output_json = output.join(filename);
        let file = File::create(output_json)?;
        serde_json::to_writer(file, &self.entities)?;

        Ok(())
    }
}

#[derive(FromArgs)]
/// Top-level command.
struct Root {
    #[argh(option)]
    /// directory containing SVGs
    svgs: Vec<PathBuf>,
    #[argh(option)]
    /// output directory
    output: PathBuf,
    #[argh(option)]
    /// maximum output width of non-scaled image
    width: i32,
    #[argh(option)]
    /// maximum output height of non-scaled image
    height: i32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root: Root = argh::from_env();
    let mut single = Combined::new(1.0, root.width, root.height);
    let mut double = Combined::new(2.0, root.width * 2, root.height * 2);

    if root.svgs.is_empty() {
        return Err("no SVG directories".into());
    }

    for svg in root.svgs {
        println!("importing SVGs from directory {}", svg.display());
        single.from_svg_files(&svg)?;
        double.from_svg_files(&svg)?;
    }

    let single = single.into_packed()?;
    let double = double.into_packed()?;

    println!("rendering image");
    single.to_png(&root.output, "")?;
    println!("rendering 2x image");
    double.to_png(&root.output, "@2x")?;

    println!("writing json");
    single.to_json(&root.output, "")?;
    println!("writing 2x json");
    double.to_json(&root.output, "@2x")?;

    Ok(())
}
