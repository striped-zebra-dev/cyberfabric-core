# File Parser Module

File parsing module for CyberFabric / ModKit.

## Overview

The `cf-file-parser` crate implements the `file-parser` module and registers REST routes.

Parsing backends currently include:

- Plain text
- HTML
- PDF
- DOCX
- Images
- Stub parser (fallback)

## Configuration

```yaml
modules:
  file-parser:
    config:
      max_file_size_mb: 100
      # Restrict local file parsing to this directory (recommended for production).
      # Paths outside this directory are rejected. Symlinks that resolve outside
      # this directory are also blocked.
      allowed_local_base_dir: /data/documents
```

### Security: Local Path Restrictions

The `parse-local` endpoints validate requested file paths before any filesystem access:

1. Paths containing `..` components are always rejected.
2. When `allowed_local_base_dir` is set, the requested path is canonicalized (symlinks resolved) and must fall under the configured directory.
3. If the setting is omitted, a warning is logged at startup. It is recommended for production deployments.

## License

Licensed under Apache-2.0.
