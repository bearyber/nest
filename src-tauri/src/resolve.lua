-- Nest's helper for DaVinci Resolve (resolve.rs). Nest runs it with Resolve's own script
-- runner: fuscript -l lua <this file> <op> <args...>
-- Every argument is text written as "x" + hex bytes, so names never become Lua source and
-- survive any command line. Each answer is one line starting with "NEST "; text in answers
-- is written the same way. Anything else fuscript prints is ignored by Nest.

local function unhex(h)
  h = (h or "x"):sub(2)
  return (h:gsub("..", function(x) return string.char(tonumber(x, 16)) end))
end

local function hex(s)
  return "x" .. (tostring(s or ""):gsub(".", function(c) return string.format("%02x", c:byte()) end))
end

local function say(...)
  print("NEST " .. table.concat({ ... }, " "))
end

-- Lists from Resolve can hold things that aren't names: keep the strings only.
local function names(list)
  local out = {}
  for _, n in pairs(list or {}) do
    if type(n) == "string" then out[#out + 1] = n end
  end
  return out
end

local function main()
  local op = arg[1]
  local a = {}
  for i = 2, #arg do a[i - 1] = unhex(arg[i]) end

  local resolve = bmd.scriptapp("Resolve")
  if not resolve then say("result", "no-resolve") return end
  local pm = resolve:GetProjectManager()
  if not pm then say("result", "failed", hex("no project manager")) return end

  local function open_name()
    local p = pm:GetCurrentProject()
    if p then return p:GetName() or "" end
    return ""
  end

  local function library(kind, db)
    if type(db) == "table" then say(kind, hex(db.DbType), hex(db.DbName)) end
  end

  -- Go to `path` ("A/B", "" = the top), making missing folders if `create`.
  local function goto_folder(path, create)
    pm:GotoRootFolder()
    for seg in string.gmatch(path or "", "[^/]+") do
      if create then pm:CreateFolder(seg) end
      if not pm:OpenFolder(seg) then
        pm:GotoRootFolder()
        return false
      end
    end
    return true
  end

  if op == "status" then
    say("result", "ready")
    return
  end

  if op == "state" or op == "libraries" then
    library("current", pm:GetCurrentDatabase())
    say("open", hex(open_name()))
    if op == "libraries" then
      for _, d in pairs(pm:GetDatabaseList() or {}) do library("library", d) end
    end
    say("result", "ok")
    return
  end

  if op == "projects" then
    local count = 0
    local function walk(path, depth)
      for _, n in ipairs(names(pm:GetProjectListInCurrentFolder())) do
        say("project", hex(path), hex(n))
        count = count + 1
      end
      if depth >= 8 or count > 5000 then return end
      for _, f in ipairs(names(pm:GetFolderListInCurrentFolder())) do
        if pm:OpenFolder(f) then
          walk(path == "" and f or (path .. "/" .. f), depth + 1)
          pm:GotoParentFolder()
        end
      end
    end
    pm:GotoRootFolder()
    walk("", 0)
    pm:GotoRootFolder()
    say("result", "ok")
    return
  end

  if op == "export" then
    local name, folder, dest = a[1], a[2], a[3]
    if not goto_folder(folder, false) then say("result", "no-folder") return end
    local ok = pm:ExportProject(name, dest, true)
    pm:GotoRootFolder()
    say("result", ok and "ok" or "failed")
    return
  end

  if op == "add" then
    local drp, name, folder, dbtype, dbname, switch = a[1], a[2], a[3], a[4], a[5], a[6]
    local before = pm:GetCurrentDatabase()
    local switched = false
    if switch == "1" then
      -- Switching libraries closes the open project: only when nothing real is open.
      local open = open_name()
      if open ~= "" and open ~= "Untitled Project" then say("result", "busy") return end
      if not pm:SetCurrentDatabase({ DbType = dbtype, DbName = dbname }) then
        say("result", "no-library")
        return
      end
      switched = true
      pm = resolve:GetProjectManager()
    end
    local result
    if not goto_folder(folder, true) then
      result = "no-folder"
    else
      local exists = false
      for _, n in ipairs(names(pm:GetProjectListInCurrentFolder())) do
        if n:lower() == name:lower() then exists = true end
      end
      if exists then
        result = "exists"
      elseif pm:ImportProject(drp, name) then
        result = "added"
      else
        result = "failed"
      end
      pm:GotoRootFolder()
    end
    if switched and type(before) == "table" then
      pm:SetCurrentDatabase({ DbType = before.DbType, DbName = before.DbName })
    end
    say("result", result)
    return
  end

  say("result", "failed", hex("unknown request"))
end

local ok, err = pcall(main)
if not ok then say("result", "failed", hex(err)) end
