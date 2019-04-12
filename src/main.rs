use cargo::{
    core::{
        dependency::Dependency,
        registry::PackageRegistry,
        resolver::Method,
        source::{MaybePackage, Source, SourceId},
        summary::Summary,
        Package, PackageId, Workspace,
    },
    ops::{resolve_with_previous, write_pkg_lockfile},
    sources::registry::RegistrySource,
    util::config::Config,
    util::errors::CargoResult,
};
use serde::Deserialize;
use std::{collections::HashSet, io::Read, path::PathBuf};

struct WrappedRegistrySource<'cfg> {
    src: RegistrySource<'cfg>,
    outer_id: SourceId,
    inner_id: SourceId,
}

impl<'cfg> WrappedRegistrySource<'cfg> {
    pub fn new(src: RegistrySource<'cfg>, outer_id: SourceId) -> Self {
        let inner_id = src.source_id();
        Self {
            src,
            inner_id,
            outer_id,
        }
    }
}

impl<'cfg> Source for WrappedRegistrySource<'cfg> {
    fn source_id(&self) -> SourceId {
        self.outer_id
    }
    fn supports_checksums(&self) -> bool {
        self.src.supports_checksums()
    }
    fn requires_precise(&self) -> bool {
        self.src.requires_precise()
    }
    fn query(&mut self, dep: &Dependency, f: &mut dyn FnMut(Summary)) -> CargoResult<()> {
        println!("query dep={:?}", dep);
        let inner_id = self.inner_id;
        let outer_id = self.outer_id;
        let mut dep = dep.clone();
        dep.set_source_id(inner_id);
        println!("replaced query dep={:?}", dep);
        self.src.query(&mut dep, &mut |summary| {
            println!("summary={:?}", summary);
            f(summary.map_source(inner_id, outer_id))
        })
    }
    fn fuzzy_query(&mut self, dep: &Dependency, f: &mut dyn FnMut(Summary)) -> CargoResult<()> {
        println!("fuzzy query dep={:?}", dep);
        let inner_id = self.inner_id;
        let outer_id = self.outer_id;
        let mut dep = dep.clone();
        dep.set_source_id(inner_id);
        println!("replaced fuzzy query dep={:?}", dep);
        self.src.fuzzy_query(&mut dep, &mut |summary| {
            println!("summary={:?}", summary);
            f(summary.map_source(inner_id, outer_id))
        })
    }
    fn update(&mut self) -> CargoResult<()> {
        self.src.update()
    }
    fn download(&mut self, package: PackageId) -> CargoResult<MaybePackage> {
        self.src.download(package)
    }
    fn finish_download(&mut self, package: PackageId, contents: Vec<u8>) -> CargoResult<Package> {
        self.src.finish_download(package, contents)
    }
    fn fingerprint(&self, pkg: &Package) -> CargoResult<String> {
        self.src.fingerprint(pkg)
    }
    fn describe(&self) -> String {
        "wrapper around local registry".into()
    }
    fn add_to_yanked_whitelist(&mut self, pkgs: &[PackageId]) {
        self.src.add_to_yanked_whitelist(pkgs)
    }
}

#[derive(Deserialize)]
pub struct LocalRegistry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Deserialize)]
pub struct GeneratorConfig {
    pub registry: Vec<LocalRegistry>,
}

fn main() {
    let gen_cfg = std::env::args().skip(1).next().unwrap();
    let mut gen_cfg = std::fs::File::open(gen_cfg).unwrap();
    let gen_cfg: GeneratorConfig = toml::from_slice(
        {
            let mut content = Vec::new();
            gen_cfg.read_to_end(&mut content).unwrap();
            content
        }
        .as_slice(),
    )
    .unwrap();

    let cfg = Config::default().unwrap();
    let cwd = std::env::current_dir().unwrap();
    let ws = Workspace::new(&cwd.join("Cargo.toml"), &cfg).unwrap();
    let yanked_whitelist = HashSet::new();
    let mut regs: Vec<_> = gen_cfg
        .registry
        .iter()
        .map(|&LocalRegistry { ref name, ref path }| {
            let inner_source_id = SourceId::for_local_registry(path).unwrap();
            WrappedRegistrySource::new(
                RegistrySource::local(inner_source_id, path, &yanked_whitelist, &cfg),
                SourceId::alt_registry(&cfg, name.as_str()).unwrap(),
            )
        })
        .collect();
    {
        let mut registry = PackageRegistry::new(&cfg).unwrap();
        regs.iter_mut()
            .for_each(|reg| registry.add_override(Box::new(reg)));
        let resolve = resolve_with_previous(
            &mut registry,
            &ws,
            Method::Everything,
            None,
            None,
            &[],
            true,
            true,
        )
        .unwrap();
        write_pkg_lockfile(&ws, &resolve).unwrap();
    }
}
