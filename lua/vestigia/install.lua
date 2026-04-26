local M = {}

local DEFAULT_REPOSITORY = "jeafleohj/vestigia"

local function plugin_root()
  local source = debug.getinfo(1, "S").source
  local script_path = source:sub(1, 1) == "@" and source:sub(2) or source

  return vim.fn.fnamemodify(script_path, ":p:h:h:h")
end

local function join_path(...)
  return table.concat({ ... }, "/")
end

local function uname()
  return (vim.uv or vim.loop).os_uname()
end

local function normalized_arch()
  local machine = uname().machine

  if machine == "arm64" then
    return "aarch64"
  end

  return machine
end

local function target_info()
  local arch = normalized_arch()
  local is_mac = vim.fn.has("macunix") == 1
  local is_linux = vim.fn.has("unix") == 1 and not is_mac

  local is_supported = (is_mac and arch == "aarch64")
      or (is_linux and (arch == "x86_64" or arch == "aarch64"))

  if not is_supported then
    error("Vestigia does not have a prebuilt binary for this platform yet")
  end

  local extension = is_mac and "dylib" or "so"
  local target = is_mac and "apple-darwin" or "unknown-linux-gnu"

  return {
    asset = string.format("vestigia-nvim-%s-%s.tar.gz", arch, target),
    library = string.format("libvestigia_nvim.%s", extension),
  }
end

local function release_url(repository, version, asset)
  if version == nil or version == "latest" then
    return ("https://github.com/%s/releases/latest/download/%s"):format(repository, asset)
  end

  return ("https://github.com/%s/releases/download/%s/%s"):format(repository, version, asset)
end

local function run(command)
  if vim.system then
    local result = vim.system(command, { text = true }):wait()

    if result.code ~= 0 then
      error(vim.trim(result.stderr or result.stdout or "command failed"))
    end

    return
  end

  local output = vim.fn.system(command)

  if vim.v.shell_error ~= 0 then
    error(vim.trim(output))
  end
end

function M.install(opts)
  opts = opts or {}

  if vim.fn.executable("curl") == 0 then
    error("Vestigia installer requires curl")
  end

  local info = target_info()
  local root = plugin_root()
  local lib_dir = join_path(root, "lib")
  local destination = join_path(lib_dir, info.library)
  local archive = join_path(lib_dir, info.asset)
  local temporary = archive .. ".tmp"

  if vim.fn.filereadable(destination) == 1 and not opts.force then
    return destination
  end

  vim.fn.mkdir(lib_dir, "p")
  vim.fn.delete(temporary)

  local url = release_url(
    opts.repository or DEFAULT_REPOSITORY,
    opts.version or "latest",
    opts.asset or info.asset
  )

  run({
    "curl",
    "--fail",
    "--location",
    "--silent",
    "--show-error",
    "--retry",
    "3",
    "--output",
    temporary,
    url,
  })

  vim.fn.rename(temporary, archive)

  run({
    "tar",
    "-xzf",
    archive,
    "-C",
    lib_dir,
    info.library,
  })

  vim.fn.delete(archive)

  return destination
end

return M
