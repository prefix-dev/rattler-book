local lumen = {}

-- Generate a thumbnail of `path` at `size` pixels on the longest side.
-- Writes `<base>_thumb.<ext>` next to the input and returns its path.
-- Shells out to ImageMagick's `magick` CLI.
function lumen.thumbnail(path, size)
    local base = path:match("(.+)%.[^.]+$") or path
    local ext = path:match(".+(%.[^.]+)$") or ""
    local out = base .. "_thumb" .. ext

    local cmd = string.format(
        "magick %q -thumbnail %dx%d %q",
        path, size, size, out
    )
    local ok, _, code = os.execute(cmd)
    if not ok then
        error(string.format("imagemagick failed (%s) for %s", tostring(code), path))
    end

    print(string.format("wrote %s (%dpx thumbnail of %s)", out, size, path))
    return out
end

return lumen
