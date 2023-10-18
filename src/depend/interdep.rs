use crate::pkgbuild::{PKGBUILD, PKGBUILDs};

struct DepNode<'a> {
    pkgbuild: &'a PKGBUILD,
    wants: Vec<&'a PKGBUILD>,
    // deps: Vec<&'a str>,
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
                // deps
            })
        }
        Ok(Self{nodes})
    }

    fn split(mut self) -> Result<Vec<Vec<&'a PKGBUILD>>, ()> {
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
                return Err(())
            }
            self.nodes = nodes;
            layers.push(layer);
        }
        println!("Split PKGBUILDs into {} layers:", layers.len());
        let mut pkgbuild_layers = vec![];
        for (layer_id, layer) in 
            layers.iter().enumerate() 
        {
            let mut pkgbuild_layer = vec![];
            print!("Layer {}:", layer_id);
            for node in layer.iter() {
                print!(" '{}'", &node.pkgbuild.base);
                pkgbuild_layer.push(node.pkgbuild);
            }
            println!();
            pkgbuild_layers.push(pkgbuild_layer)
        }
        Ok(pkgbuild_layers)
    }
}

pub(crate) fn split_pkgbuilds<'a>(pkgbuilds: &'a PKGBUILDs) 
    -> Result<Vec<Vec<&'a PKGBUILD>>, ()> 
{
    DepNodes::from_pkgbuilds(pkgbuilds)?.split()
}