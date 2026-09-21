# Shared font sources

The terminal, gallery and GUI fixtures share the complete Shure Tech Mono Nerd Font. Adwaita Mono and Noto Sans exercise composite glyphs and proportional kerning in the converter tests. These dependencies belong here rather than to an individual example.

[sources.json](sources.json) pins immutable upstream revisions and SHA-256 checksums. [The downloader](../../tools/font_sources.py) verifies cached files, downloads missing or corrupt files, and publishes only verified content. Noto Sans is instantiated at the pinned regular weight and width with the repository's pinned FontTools; its resulting checksum is also verified. This directory contains source pins and build documentation; licence and provenance notices live in [licences/fonts](../../licences/fonts/README.md). Font binaries are downloaded or generated under ignored `target/` directories; the local `.gitignore` also excludes accidental binary copies here.

`python tools/ipp.py build font-assets` downloads sources into ignored `target/font-sources/` and converts the complete application font into `target/font-assets/shure-tech-mono.ippf`. Gallery and Surface builds declare this prerequisite, so clean local and GitHub Actions builds fetch fonts automatically. A populated, valid source cache supports offline rebuilds; an empty cache requires network access. Deployed applications serve the built IPPF asset locally and do not contact the font providers.

The gallery site packager includes the [Shure notices](../../licences/fonts/README.md) with its shared runtime font. Preserve the full glyph set for editable GUI text; changing a source or conversion requires updating the pin and validating its consumers.
