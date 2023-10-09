use super::pkgbuild::{PKGBUILD, PKGBUILDs};

struct DepNode<'a> {
    pkgbuild: &'a PKGBUILD,
    wants: Vec<&'a PKGBUILD>,
    deps: Vec<&'a str>,
}

struct DepNodes<'a> {
    nodes: Vec<DepNode<'a>>
}

impl<'a> DepNodes<'a>  {
    fn from_pkgbuilds(pkgbuilds: &'a PKGBUILDs) -> Result<Self, ()> {
        let mut nodes = vec![];
        for pkgbuild in pkgbuilds.0.iter() {
            let mut wants = vec![];
            let mut deps = vec![];
            for pkgbuild_target in pkgbuilds.0.iter() {
                if std::ptr::eq(pkgbuild, pkgbuild_target) {
                    continue
                }
                if let Some(dep) = pkgbuild.wants(pkgbuild_target) {
                    if deps.contains(&dep) {
                        eprintln!("'{}' is provided by multiple PKGBUILDs",dep);
                        return Err(())
                    } else {
                        wants.push(pkgbuild_target);
                        deps.push(&dep)
                    }
                }
            }
            nodes.push(DepNode{
                pkgbuild,
                wants,
                deps
            })
        }
        Ok(Self{nodes})
    }

    fn split(mut self) {
        let mut layers: Vec<Vec<DepNode>> = vec![];
        while ! self.nodes.is_empty() {
            if let Some(layer) = layers.last() {
                println!("Removing deps in last layer");
                for node in self.nodes.iter_mut() {
                    for node_old in layer.iter() {
                        node.wants.retain(|pkgbuild|
                            !std::ptr::eq(*pkgbuild, node_old.pkgbuild));
                    }
                }
            }
            let (layer, nodes)
                : (Vec<DepNode>, Vec<DepNode>) 
                = self.nodes.into_iter().partition(
                    |node|node.wants.is_empty());
            if layer.is_empty() {
                eprintln!("Failed to split dep layers more, current layer is \
                    empty, remaining nodes: {}", nodes.len());
                return
            }
            self.nodes = nodes;
            layers.push(layer);
        }
        println!("Split PKGBUILDs into {} layers:", layers.len());
        for (layer_id, layer) in 
            layers.iter().enumerate() 
        {
            println!("Layer {}:", layer_id);
            for node in layer.iter() {
                println!(" '{}'", &node.pkgbuild.base)
            }

        }
    }
}