use std::fs;
use std::path::{Path, PathBuf};

use storage::{Backend, CachedBackend, Component};
use core::{CliError, LalResult, output};

fn is_cached<T: Backend>(backend: &T, name: &str, version: u32, env: Option<&str>) -> bool {
    get_cache_dir(backend, name, version, env).is_dir()
}

fn get_cache_dir<T: Backend>(backend: &T, name: &str, version: u32, env: Option<&str>) -> PathBuf {
    let cache = backend.get_cache_dir();
    let pth = Path::new(&cache);
    match env {
            None => pth.join("globals"),
            Some(e) => pth.join("environments").join(e),
        }
        .join(name)
        .join(version.to_string())
}

fn store_tarball<T: Backend>(backend: &T,
                             name: &str,
                             version: u32,
                             env: Option<&str>)
                             -> Result<(), CliError> {
    // 1. mkdir -p cacheDir/$name/$version
    let destdir = get_cache_dir(backend, name, version, env);
    if !destdir.is_dir() {
        fs::create_dir_all(&destdir)?;
    }
    // 2. stuff $PWD/$name.tar in there
    let tarname = [name, ".tar"].concat();
    let dest = Path::new(&destdir).join(&tarname);
    let src = Path::new(".").join(&tarname);
    if !src.is_file() {
        return Err(CliError::MissingTarball);
    }
    debug!("Move {:?} -> {:?}", src, dest);
    fs::copy(&src, &dest)?;
    fs::remove_file(&src)?;

    Ok(())
}

// helper for the unpack_ functions
fn extract_tarball_to_input(tarname: PathBuf, component: &str) -> LalResult<()> {
    use tar::Archive;
    use flate2::read::GzDecoder;

    let data = fs::File::open(tarname)?;
    let decompressed = GzDecoder::new(data)?; // decoder reads data
    let mut archive = Archive::new(decompressed); // Archive reads decoded

    let extract_path = Path::new("./INPUT").join(component);
    let _ = fs::remove_dir_all(&extract_path); // remove current dir if exists
    fs::create_dir_all(&extract_path)?;
    archive.unpack(&extract_path)?;
    Ok(())
}

/// Cacheable trait implemented for all Backends.
///
/// As long as we have the Backend trait implemented, we can add a caching layer
/// around this, which implements the basic compression ops and file gymnastics.
///
/// Most subcommands should be OK with just using this trait rather than using
/// `Backend` directly as this does the stuff you normally would want done.
impl<T> CachedBackend for T
    where T: Backend
{
    /// Locate a proper component, downloading it and caching if necessary
    fn retrieve_published_component(&self,
                                    name: &str,
                                    version: Option<u32>,
                                    env: Option<&str>)
                                    -> LalResult<(PathBuf, Component)> {
        trace!("Locate component {}", name);

        let component = self.get_tarball_url(name, version, env)?;

        if !is_cached(self, &component.name, component.version, env) {
            // download to PWD then move it to stash immediately
            let local_tarball = Path::new(".").join(format!("{}.tar", name));
            self.raw_download(&component.tarball, &local_tarball)?;
            store_tarball(self, name, component.version, env)?;
        }
        assert!(is_cached(self, &component.name, component.version, env),
                "cached component");

        trace!("Fetching {} from cache", name);
        let tarname = get_cache_dir(self, &component.name, component.version, env)
            .join(format!("{}.tar", name));
        Ok((tarname, component))
    }

    // basic functionality for `fetch`/`update`
    fn unpack_published_component(&self,
                                  name: &str,
                                  version: Option<u32>,
                                  env: Option<&str>)
                                  -> LalResult<Component> {
        let (tarname, component) = self.retrieve_published_component(name, version, env)?;

        debug!("Unpacking tarball {} for {}",
               tarname.to_str().unwrap(),
               component.name);
        extract_tarball_to_input(tarname, name)?;

        Ok(component)
    }

    /// helper for `update`
    fn unpack_stashed_component(&self, name: &str, code: &str) -> LalResult<()> {
        let tarpath = self.retrieve_stashed_component(name, code)?;

        extract_tarball_to_input(tarpath, name)?;
        Ok(())
    }

    /// helper for unpack_, `export`
    fn retrieve_stashed_component(&self, name: &str, code: &str) -> LalResult<PathBuf> {
        let tarpath = Path::new(&self.get_cache_dir())
            .join("stash")
            .join(name)
            .join(code)
            .join(format!("{}.tar.gz", name));
        if !tarpath.is_file() {
            return Err(CliError::MissingStashArtifact(format!("{}/{}", name, code)));
        }
        Ok(tarpath)
    }

    // helper for `stash`
    fn stash_output(&self, name: &str, code: &str) -> LalResult<()> {
        let destdir = Path::new(&self.get_cache_dir())
            .join("stash")
            .join(name)
            .join(code);
        debug!("Creating {:?}", destdir);
        fs::create_dir_all(&destdir)?;

        // Tar it straight into destination
        output::tar(&destdir.join(format!("{}.tar.gz", name)))?;

        // Copy the lockfile there for users inspecting the stashed folder
        // NB: this is not really needed, as it's included in the tarball anyway
        fs::copy("./OUTPUT/lockfile.json", destdir.join("lockfile.json"))?;
        Ok(())
    }
}