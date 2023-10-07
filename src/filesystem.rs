use std::{path::Path, fs::{read_dir, remove_dir, remove_file, remove_dir_all}};


// build/*/pkg being 0111 would cause remove_dir_all() to fail, in this case
// we use our own implementation
pub(crate) fn remove_dir_recursively<P: AsRef<Path>>(dir: P) 
    -> Result<(), std::io::Error>
{
    for entry in read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_symlink() && path.is_dir() {
            let er = 
                remove_dir_recursively(&path);
            match remove_dir(&path) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!(
                        "Failed to remove subdir '{}' recursively: {}", 
                        path.display(), e);
                    if let Err(e) = er {
                        eprintln!("Subdir failure: {}", e)
                    }
                    return Err(e);
                },
            }
        } else {
            remove_file(&path)?
        }
    }
    Ok(())
}


pub(crate) fn remove_dir_all_try_best<P: AsRef<Path>>(dir: P) 
    -> Result<(), ()>
{
    println!("Removing dir '{}' recursively...", dir.as_ref().display());
    match remove_dir_all(&dir) {
        Ok(_) => return Ok(()),
        Err(e) => {
            eprintln!("Failed to remove dir '{}' recursively naively: {}", 
                dir.as_ref().display(), e);
        },
    }
    remove_dir_recursively(&dir).or_else(|e|{
        eprintln!("Failed to remove dir '{}' recursively: {}", 
            dir.as_ref().display(), e);
        Err(())
    })?;
    remove_dir(&dir).or_else(|e|{
        eprintln!("Failed to remove dir '{}' itself: {}",
            dir.as_ref().display(), e);
        Err(())
    })?;
    println!("Removed dir '{}' recursively", dir.as_ref().display());
    Ok(())

}