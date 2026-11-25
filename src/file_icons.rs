use gpui_kit::{Pixels, Styled as _, Svg, rgb, svg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileIconKind {
    Folder,
    FolderOpen,
    FolderSrc,
    FolderGit,
    FolderConfig,
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Json,
    Yaml,
    Toml,
    Markdown,
    Shell,
    Html,
    Css,
    Docker,
    Git,
    Database,
    Lock,
    Image,
    Archive,
    Log,
    DefaultFile,
}

impl FileIconKind {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Folder => "icons/material/folder.svg",
            Self::FolderOpen => "icons/material/folder-open.svg",
            Self::FolderSrc => "icons/material/folder-src.svg",
            Self::FolderGit => "icons/material/folder-git.svg",
            Self::FolderConfig => "icons/material/folder-config.svg",
            Self::Rust => "icons/material/rust.svg",
            Self::TypeScript => "icons/material/typescript.svg",
            Self::JavaScript => "icons/material/javascript.svg",
            Self::Python => "icons/material/python.svg",
            Self::Go => "icons/material/go.svg",
            Self::Json => "icons/material/json.svg",
            Self::Yaml => "icons/material/yaml.svg",
            Self::Toml => "icons/material/toml.svg",
            Self::Markdown => "icons/material/markdown.svg",
            Self::Shell => "icons/material/shell.svg",
            Self::Html => "icons/material/html.svg",
            Self::Css => "icons/material/css.svg",
            Self::Docker => "icons/material/docker.svg",
            Self::Git => "icons/material/git.svg",
            Self::Database => "icons/material/database.svg",
            Self::Lock => "icons/material/lock.svg",
            Self::Image => "icons/material/image.svg",
            Self::Archive => "icons/material/archive.svg",
            Self::Log => "icons/material/log.svg",
            Self::DefaultFile => "icons/material/file.svg",
        }
    }

    pub const fn color(self) -> u32 {
        match self {
            Self::Folder | Self::FolderOpen => 0x90a4ae,
            Self::FolderSrc => 0x42a5f5,
            Self::FolderGit => 0xf05032,
            Self::FolderConfig => 0x26a69a,
            Self::Rust => 0xdea584,
            Self::TypeScript => 0x3178c6,
            Self::JavaScript => 0xf1e05a,
            Self::Python => 0x4b8bbe,
            Self::Go => 0x00add8,
            Self::Json => 0xfbc02d,
            Self::Yaml => 0xab47bc,
            Self::Toml => 0xff7043,
            Self::Markdown => 0x42a5f5,
            Self::Shell => 0x66bb6a,
            Self::Html => 0xe44d26,
            Self::Css => 0x42a5f5,
            Self::Docker => 0x2496ed,
            Self::Git => 0xf05032,
            Self::Database => 0xffca28,
            Self::Lock => 0x8d6e63,
            Self::Image => 0x26a69a,
            Self::Archive => 0xef5350,
            Self::Log => 0x78909c,
            Self::DefaultFile => 0x90a4ae,
        }
    }

    pub fn render(self, size: Pixels) -> Svg {
        svg()
            .path(self.path())
            .size(size)
            .flex_shrink_0()
            .text_color(rgb(self.color()))
    }
}

pub fn classify_file(name: &str, is_dir: bool, is_expanded: bool) -> FileIconKind {
    let lower = name.to_lowercase();
    if is_dir {
        if is_expanded {
            return FileIconKind::FolderOpen;
        }
        return match lower.as_str() {
            "src" | "lib" | "app" => FileIconKind::FolderSrc,
            ".git" | ".github" => FileIconKind::FolderGit,
            ".ssh" | "config" | "etc" | ".vscode" => FileIconKind::FolderConfig,
            _ => FileIconKind::Folder,
        };
    }

    match lower.as_str() {
        "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" => return FileIconKind::Docker,
        ".gitignore" | ".gitattributes" | ".gitmodules" => return FileIconKind::Git,
        "cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => {
            return FileIconKind::Lock;
        }
        "makefile" => return FileIconKind::Shell,
        _ => {}
    }

    let ext = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match ext {
        "rs" => FileIconKind::Rust,
        "ts" | "tsx" | "mts" | "cts" => FileIconKind::TypeScript,
        "js" | "jsx" | "mjs" | "cjs" => FileIconKind::JavaScript,
        "py" | "pyi" => FileIconKind::Python,
        "go" => FileIconKind::Go,
        "json" | "jsonc" => FileIconKind::Json,
        "yml" | "yaml" => FileIconKind::Yaml,
        "toml" | "ini" | "conf" | "cfg" | "env" => FileIconKind::Toml,
        "md" | "markdown" | "mdx" => FileIconKind::Markdown,
        "sh" | "bash" | "zsh" | "fish" => FileIconKind::Shell,
        "html" | "htm" => FileIconKind::Html,
        "css" | "scss" | "sass" | "less" => FileIconKind::Css,
        "sql" | "db" | "sqlite" | "sqlite3" => FileIconKind::Database,
        "lock" => FileIconKind::Lock,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" => FileIconKind::Image,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" => FileIconKind::Archive,
        "log" => FileIconKind::Log,
        _ => FileIconKind::DefaultFile,
    }
}

pub fn svg_bytes(path: &str) -> Option<&'static [u8]> {
    match path {
        "icons/material/folder.svg" | "icons/material/folder-src.svg" | "icons/material/folder-git.svg" | "icons/material/folder-config.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M10 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2h-8l-2-2z"/></svg>"##,
        ),
        "icons/material/folder-open.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M20 6h-8l-2-2H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2zm0 12H4V8h16v10z"/></svg>"##,
        ),
        "icons/material/rust.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v3M12 19v3M4.93 4.93l2.12 2.12M16.95 16.95l2.12 2.12M2 12h3M19 12h3M4.93 19.07l2.12-2.12M16.95 7.05l2.12-2.12"/></svg>"##,
        ),
        "icons/material/typescript.svg" | "icons/material/javascript.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="3"/><path d="M8 10h6M11 10v7M15.5 14.5c0-1 .8-1.5 1.8-1.5s1.7.5 1.7 1.3c0 1.5-3.5 1.2-3.5 2.7 0 .8.8 1.2 1.8 1.2 1 0 1.7-.4 1.7-1.2"/></svg>"##,
        ),
        "icons/material/python.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3c-3 0-4 1.2-4 3.5V9h6v1.5H7.5C5.2 10.5 4 11.8 4 14.5S5.2 18 7.5 18H9v-2.5c0-1.5 1-2.5 2.5-2.5h5c1.3 0 2.5-1 2.5-2.5V6.5C19 4.2 17.5 3 12 3z"/><circle cx="10" cy="6" r=".8" fill="currentColor"/></svg>"##,
        ),
        "icons/material/go.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 12h6M5 9h4M3 15h5"/><circle cx="15.5" cy="12" r="4.5"/><path d="M15.5 12h3"/></svg>"##,
        ),
        "icons/material/json.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 4c-2 0-2.5 1.2-2.5 2.8v2.7c0 1.2-.8 2-2 2.5 1.2.5 2 1.3 2 2.5v2.7C5.5 18.8 6 20 8 20"/><path d="M16 4c2 0 2.5 1.2 2.5 2.8v2.7c0 1.2.8 2 2 2.5-1.2.5-2 1.3-2 2.5v2.7c0 1.6-.5 2.8-2.5 2.8"/></svg>"##,
        ),
        "icons/material/yaml.svg" | "icons/material/toml.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M4 12h10M4 17h14"/><circle cx="18" cy="12" r="2"/></svg>"##,
        ),
        "icons/material/markdown.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M6.5 15V9l2.5 3 2.5-3v6M16.5 9v6m0 0-2-2m2 2 2-2"/></svg>"##,
        ),
        "icons/material/shell.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="m7.5 9.5 3 2.5-3 2.5M12.5 14.5h4"/></svg>"##,
        ),
        "icons/material/html.svg" | "icons/material/css.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="m8 7-5 5 5 5M16 7l5 5-5 5M13.5 5l-3 14"/></svg>"##,
        ),
        "icons/material/docker.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 13h17c-.5 4-3.5 7-8.5 7S4 17 3 13z"/><path d="M6 10h3v3H6zM10 10h3v3h-3zM14 10h3v3h-3zM10 7h3v3h-3z"/></svg>"##,
        ),
        "icons/material/git.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="8" r="2.5"/><circle cx="6" cy="18" r="2.5"/><path d="M6 8.5v7M8.5 6.5c4 0 7 .5 7.5 1.5"/></svg>"##,
        ),
        "icons/material/database.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="6" rx="7" ry="3"/><path d="M5 6v12c0 1.66 3.13 3 7 3s7-1.34 7-3V6"/><path d="M5 12c0 1.66 3.13 3 7 3s7-1.34 7-3"/></svg>"##,
        ),
        "icons/material/lock.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>"##,
        ),
        "icons/material/image.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="8.5" cy="9" r="1.5"/><path d="m21 15-5-5-8 8"/></svg>"##,
        ),
        "icons/material/archive.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="4" width="16" height="4" rx="1"/><path d="M5 8v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/></svg>"##,
        ),
        "icons/material/log.svg" | "icons/material/file.svg" => Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M14 3v6h6M8 13h8M8 17h5"/></svg>"##,
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_material_file_and_folder_icons() {
        assert_eq!(classify_file("src", true, false), FileIconKind::FolderSrc);
        assert_eq!(classify_file("src", true, true), FileIconKind::FolderOpen);
        assert_eq!(classify_file("main.rs", false, false), FileIconKind::Rust);
        assert_eq!(
            classify_file("Dockerfile", false, false),
            FileIconKind::Docker
        );
        assert_eq!(
            classify_file("Cargo.lock", false, false),
            FileIconKind::Lock
        );
        assert!(svg_bytes(FileIconKind::Rust.path()).is_some());
    }
}
