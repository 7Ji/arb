use std::fmt::Display;

struct Package {
    base: String,
    names: Vec<String>,
    deps: Vec<String>,
}

impl Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PKGBUILD {} providing {:?}", &self.base, &self.names)
    }
}


fn init() -> Vec<Package> {
    let pkg_a = Package {
        base: String::from("ampart"),
        names: vec![String::from("ampart-cli"), String::from("ampart-api")],
        deps: vec![String::from("zlib")],
    };
    let pkg_b = Package {
        base: String::from("zlib"),
        names: vec![String::from("zlib"), String::from("zlib-headers")],
        deps: vec![]
    };
    let pkg_c = Package {
        base: String::from("some_app"),
        names: vec![String::from("some_app")],
        deps: vec![String::from("ampart-cli"), String::from("zlib")]
    };
    vec![pkg_a, pkg_b, pkg_c]
}

struct PackageProvide<'a> {
    base: &'a Package,
    name: &'a String,
}

impl <'a> Display for PackageProvide<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} provided by {}", self.name, self.base)
    }
}

fn to_provide<'a> (pkgs: &'a Vec<Package>) -> Vec<PackageProvide<'a>>{
    let mut provides = vec![];
    for pkg in pkgs.iter() {
        for name in pkg.names.iter() {
            provides.push(PackageProvide{
                base: pkg,
                name,
            })
        }
    }
    println!("All provides:");
    for provide in provides.iter() {
        println!("{}", &provide)
    }
    provides
}

struct DepNode<'a> {
    pkg: &'a Package,
    needs: Vec<&'a Package>,
    needed_by: Vec<&'a Package>,
}

impl <'a> Display for DepNode<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Package {} needs:", &self.pkg.base)?;
        let mut started = false;
        for need in self.needs.iter() {
            if started {
                write!(f, ",")?;
            } else {
                started = true
            }
            write!(f, " {}", need)?;
        }
        writeln!(f)?;
        write!(f, "Package {} needed by:", &self.pkg.base)?;
        let mut started = false;
        for need in self.needed_by.iter() {
            if started {
                write!(f, ",")?;
            } else {
                started = true
            }
            write!(f, " {}", need)?;
        }
        Ok(())
    }
}

fn print_layers(layers: Vec<Vec<DepNode>>) {
    println!("All layers:");
    for (layer_id, layer) in layers.iter().enumerate() {
        println!("Layer {}:", layer_id);
        for node in layer.iter() {
            println!("{}", node);
        }
    }
}

fn parse<'a> (pkgs: &'a Vec<Package>) {
    let mut nodes = vec![];
    for pkg in pkgs.iter() {
        let mut needs = vec![];
        for dep in pkg.deps.iter() {
            let mut found = false;
            for pkg_target in pkgs.iter() {
                if std::ptr::eq(pkg, pkg_target) {
                    continue
                }
                if pkg_target.names.contains(dep) {
                    if ! found {
                        needs.push(pkg_target);
                        found = true;
                        // break
                    } else {
                        eprintln!("Warning: dep duplicated! {} provided by multiple packages", dep);
                    }
                }
            }
        }
        let mut needed_by = vec![];
        for pkg_target in pkgs.iter() {
            let mut found = false;
            for dep in pkg_target.deps.iter() {
                if pkg.names.contains(dep) {
                    if ! found {
                        needed_by.push(pkg_target);
                        found = true;
                        // break
                    } else {
                        eprintln!("Warning: dep duplicated! {} provided by multiple packages", dep);
                    }
                }
            }
        }
        nodes.push(DepNode{
            pkg,
            needs,
            needed_by,
        })
    }
    println!("All nodes:");
    for node in nodes.iter() {
        println!("{}", node);
    }
    let mut node_layers: Vec<Vec<DepNode>> = vec![];
    while ! nodes.is_empty() {
        if let Some(layer) = node_layers.last() {
            println!("Removing deps in last layer");
            for node in nodes.iter_mut() {
                for node_old in layer.iter() {
                    node.needs.retain(|dep|!std::ptr::eq(*dep, node_old.pkg));
                }
            }
        }
        let (layer, remaining_nodes): (Vec<DepNode>, Vec<DepNode>) 
            = nodes.into_iter().partition(|node|node.needs.is_empty());
        if layer.is_empty() {
            eprintln!("Failed to split dep layers more, current layer is empty, remaining nodes: {}", remaining_nodes.len());
            for node in remaining_nodes.iter() {
                println!("{}", node);
            }
            print_layers(node_layers);
            return
        }
        nodes = remaining_nodes;
        node_layers.push(layer);
        println!("Split once")
    }
    print_layers(node_layers);
}

fn main() {
    let pkgs = init();
    let provides = to_provide(&pkgs);
    parse(&pkgs);
    // println!("Provides: {:?}", provides);
    // parse(&pkgs)
}
