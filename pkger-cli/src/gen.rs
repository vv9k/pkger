use crate::opts::GenRecipeOpts;
use pkger_core::recipe::{DebRep, MetadataRep, PkgRep, RecipeRep, RpmRep};

use serde_yaml::{Mapping, Value as YamlValue};
use tracing::{info_span, trace, warn};

pub fn recipe(opts: Box<GenRecipeOpts>) -> RecipeRep {
    let span = info_span!("gen-recipe");
    let _enter = span.enter();
    trace!(opts = ?opts);

    let git = if let Some(url) = opts.git_url {
        let mut git_src = Mapping::new();
        git_src.insert(YamlValue::from("url"), YamlValue::from(url));
        if let Some(branch) = opts.git_branch {
            git_src.insert(YamlValue::from("branch"), YamlValue::from(branch));
        }
        YamlValue::Mapping(git_src)
    } else {
        YamlValue::Null
    };

    let mut env = Mapping::new();
    if let Some(env_str) = opts.env {
        for kv in env_str.split(',') {
            let mut kv_split = kv.split('=');
            if let Some(k) = kv_split.next() {
                if let Some(v) = kv_split.next() {
                    if let Some(entry) = env.insert(YamlValue::from(k), YamlValue::from(v)) {
                        warn!(key = k, old = ?entry.as_str(), new = v, "key already exists, overwriting")
                    }
                } else {
                    warn!(entry = ?kv, "env entry missing a `=`");
                }
            } else {
                warn!(entry = kv, "env entry missing a key or `=`");
            }
        }
    }

    macro_rules! vec_as_deps {
        ($it:expr) => {{
            let vec = $it.into_iter().map(YamlValue::from).collect::<Vec<_>>();
            if vec.is_empty() {
                YamlValue::Null
            } else {
                YamlValue::Sequence(vec)
            }
        }};
    }

    let deb = DebRep {
        priority: opts.priority,
        built_using: opts.built_using,
        essential: opts.essential,

        pre_depends: vec_as_deps!(opts.pre_depends),
        recommends: vec_as_deps!(opts.recommends),
        suggests: vec_as_deps!(opts.suggests),
        breaks: vec_as_deps!(opts.breaks),
        replaces: vec_as_deps!(opts.replaces.clone()),
        enhances: vec_as_deps!(opts.enchances),

        postinst_script: None,
    };

    let rpm = RpmRep {
        obsoletes: vec_as_deps!(opts.obsoletes),
        vendor: opts.vendor,
        icon: opts.icon,
        summary: opts.summary,
        auto_req_prov: None,
        pre_script: None,
        post_script: None,
        preun_script: None,
        postun_script: None,
        config_noreplace: opts.config_noreplace,
    };

    let pkg = PkgRep {
        install: opts.install_script,
        backup: opts.backup_files.unwrap_or_default(),
        replaces: vec_as_deps!(opts.replaces),
        optdepends: opts.optdepends.unwrap_or_default(),
    };

    let metadata = MetadataRep {
        name: opts.name,
        version: opts.version.unwrap_or_else(|| "1.0.0".to_string()),
        description: opts.description.unwrap_or_else(|| "missing".to_string()),
        license: opts.license.unwrap_or_else(|| "missing".to_string()),
        all_images: false,
        images: vec![],

        maintainer: opts.maintainer,
        url: opts.url,
        arch: opts.arch,
        source: opts.source,
        git,
        skip_default_deps: opts.skip_default_deps,
        exclude: opts.exclude,
        group: opts.group,
        release: opts.release,
        epoch: opts.epoch,

        build_depends: vec_as_deps!(opts.build_depends),
        depends: vec_as_deps!(opts.depends),
        conflicts: vec_as_deps!(opts.conflicts),
        provides: vec_as_deps!(opts.provides),
        patches: vec_as_deps!(opts.patches),

        deb: Some(deb),
        rpm: Some(rpm),
        pkg: Some(pkg),
        apk: None,
    };

    RecipeRep {
        metadata,
        env: if env.is_empty() { None } else { Some(env) },
        configure: None,
        build: Default::default(),
        install: None,
    }
}
