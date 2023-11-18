// Pacman config parsing
use std::fmt::{
        Display,
        Write,
    };

use crate::error::{
        Error,
        Result
    };

pub(crate) struct Section<'a> {
    pub(crate) name: &'a str,
    pub(crate) lines: Vec<&'a str>,
}

impl<'a> Display for Section<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("[{}]\n", self.name))?;
        for line in self.lines.iter() {
            f.write_str(line)?;
            f.write_char('\n')?;
        }
        Ok(())
    }
}

pub(crate) struct Config<'a> {
    pub(crate) options: Section<'a>,
    pub(crate) repos: Vec<Section<'a>>
}

impl<'a> Display for Config<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.options.fmt(f)?;
        for repo in self.repos.iter() {
            repo.fmt(f)?;
        }
        Ok(())
    }
}

impl<'a> Config<'a> {
    pub(crate) fn from_pacman_conf_content(content: &'a str)
        -> Result<Self>
    {
        let mut sections = vec![];
        let mut section = None;
        for mut line in content.lines() {
            line = line.trim();
            if line.is_empty() { continue }
            if line.starts_with('#') { continue }
            if line.starts_with('[') && line.ends_with(']') {
                let name = &line[1..(line.len() - 1)];
                sections.push(Section{ name, lines: vec![] });
                section = sections.last_mut()
            } else if let Some(section) = &mut section {
                section.lines.push(line)
            }
        }
        let (mut options, repos)
            : (Vec<Section>, _)
                = sections.into_iter().partition(
                    |section|section.name == "options");
        if options.len() != 1 {
            log::error!("Failed to find options section, please check your \
                pacman config");
            return Err(Error::InvalidConfig)
        }
        Ok(Self {
            options: options.swap_remove(0),
            repos,
        })
    }

    pub(crate) fn with_cusrepo(&self, name: &str, path: &str) -> String {
        let mut content = self.options.to_string();
        content.push_str(
            &format!("[{}]\nServer = file://{}\n", name, path));
        for repo in self.repos.iter() {
            content.push_str(repo.to_string().as_str())
        }
        content
    }
}
