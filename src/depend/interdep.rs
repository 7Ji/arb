use crate::pkgbuild::{
        PKGBUILD,
        PKGBUILDs
    };

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
                        log::error!("'{}' is provided by multiple PKGBUILDs",
                                    dep);
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
                log::info!("Removing deps in last layer");
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
                log::error!("Failed to split dep layers more, current layer is \
                    empty, remaining nodes: {}", nodes.len());
                return Err(())
            }
            self.nodes = nodes;
            layers.push(layer);
        }
        log::info!("Split PKGBUILDs into {} layers:", layers.len());
        let mut pkgbuild_layers = vec![];
        for (layer_id, layer) in
            layers.iter().enumerate()
        {
            let mut pkgbuild_layer = vec![];
            let mut line = format!("Layer {}:", layer_id);
            for node in layer.iter() {
                line.push_str(
                    format!(" '{}'", &node.pkgbuild.base).as_str());
                pkgbuild_layer.push(node.pkgbuild);
            }
            line.push('\n');
            log::info!("{}", line);
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