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
  -- So Nest still sees what happened if the run is stopped later.
  pcall(function() io.stdout:flush() end)
end

-- Lists from Resolve can hold things that aren't names: keep the strings only.
local function names(list)
  local out = {}
  for _, n in pairs(list or {}) do
    if type(n) == "string" then out[#out + 1] = n end
  end
  return out
end

local function count(list)
  local n = 0
  for _ in pairs(list or {}) do n = n + 1 end
  return n
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

  -- What the open project holds: timelines, clips and folders at the top of its media pool.
  -- nil = couldn't tell (then it counts as real work). No project open = 0 0 0.
  local function contents()
    local ok, t, c, f = pcall(function()
      local p = pm:GetCurrentProject()
      if not p then return 0, 0, 0 end
      local root = p:GetMediaPool():GetRootFolder()
      return p:GetTimelineCount(), count(root:GetClipList()), count(root:GetSubFolderList())
    end)
    if ok and type(t) == "number" and type(c) == "number" and type(f) == "number" then
      return t, c, f
    end
    return nil
  end

  -- Nothing real open: switching libraries (which closes the open project) loses nothing.
  -- Decided from what the project holds, never from its name (any language, any name).
  local function nothing_real_open()
    local t, c, f = contents()
    return t == 0 and c == 0 and f == 0
  end

  local function library(kind, db)
    if type(db) == "table" then
      say(kind, hex(db.DbType), hex(db.DbName), hex(db.IpAddress or ""))
    end
  end

  -- A library from three arguments (type, name, address; "" = none).
  local function db_of(dbtype, dbname, ip)
    local db = { DbType = dbtype, DbName = dbname }
    if ip and ip ~= "" then db.IpAddress = ip end
    return db
  end

  -- The project manager's folder now, as names from the top ("" at the top).
  local function here()
    local path = {}
    for _ = 1, 32 do
      local name = pm:GetCurrentFolder()
      if not pm:GotoParentFolder() then break end
      table.insert(path, 1, name)
    end
    return path
  end

  local function back_to(path)
    pm:GotoRootFolder()
    for _, seg in ipairs(path or {}) do
      if not pm:OpenFolder(seg) then break end
    end
  end

  -- Go to `path` ("A/B", "" = the top), making missing folders if `create` (only when a
  -- folder can't be opened, so an existing one is never touched).
  local function goto_folder(path, create)
    pm:GotoRootFolder()
    for seg in string.gmatch(path or "", "[^/]+") do
      if not pm:OpenFolder(seg) then
        if not (create and pm:CreateFolder(seg) and pm:OpenFolder(seg)) then
          pm:GotoRootFolder()
          return false
        end
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
    local t, c, f = contents()
    if t then
      say("contents", string.format("%d", t), string.format("%d", c), string.format("%d", f))
    end
    if op == "libraries" then
      for _, d in pairs(pm:GetDatabaseList() or {}) do library("library", d) end
    end
    say("result", "ok")
    return
  end

  if op == "projects" then
    local start = here()
    local count_ = 0
    local function walk(path, depth)
      for _, n in ipairs(names(pm:GetProjectListInCurrentFolder())) do
        say("project", hex(path), hex(n))
        count_ = count_ + 1
      end
      if depth >= 8 or count_ > 5000 then return end
      for _, f in ipairs(names(pm:GetFolderListInCurrentFolder())) do
        if pm:OpenFolder(f) then
          walk(path == "" and f or (path .. "/" .. f), depth + 1)
          pm:GotoParentFolder()
        end
      end
    end
    pm:GotoRootFolder()
    walk("", 0)
    back_to(start)
    say("result", "ok")
    return
  end

  if op == "export" then
    local name, folder, dest = a[1], a[2], a[3]
    local start = here()
    if not goto_folder(folder, false) then
      back_to(start)
      say("result", "no-folder")
      return
    end
    local ok = pm:ExportProject(name, dest, true)
    back_to(start)
    say("result", ok and "ok" or "failed")
    return
  end

  -- Switch back to a library Nest left Resolve in (only with nothing real open).
  if op == "switch" then
    if not nothing_real_open() then say("result", "busy") return end
    if pm:SetCurrentDatabase(db_of(a[1], a[2], a[3])) then
      say("result", "ok")
    else
      say("result", "no-library")
    end
    return
  end

  if op == "add" then
    local drp, name, folder, dbtype, dbname, switch, ip = a[1], a[2], a[3], a[4], a[5], a[6], a[7]
    local before = pm:GetCurrentDatabase()
    local switched = false
    local start = nil
    if switch == "1" then
      -- Switching libraries closes the open project: only when nothing real is open.
      if not nothing_real_open() then say("result", "busy") return end
      if type(before) ~= "table" then
        say("result", "failed", hex("Resolve didn't say which library is open"))
        return
      end
      if not pm:SetCurrentDatabase(db_of(dbtype, dbname, ip)) then
        say("result", "no-library")
        return
      end
      switched = true
      say("switched")
      pm = resolve:GetProjectManager()
    else
      start = here()
    end

    -- Everything after the switch runs on its own, so the switch back always happens.
    local ok, result = pcall(function()
      if not goto_folder(folder, true) then return "no-folder" end
      local exists = false
      for _, n in ipairs(names(pm:GetProjectListInCurrentFolder())) do
        if n:lower() == name:lower() then exists = true end
      end
      if exists then return "exists" end
      if pm:ImportProject(drp, name) then return "added" end
      return "failed"
    end)

    if switched then
      -- Exactly the library Resolve had (every field it gave), checked.
      local back_ok, back = pcall(function()
        return resolve:GetProjectManager():SetCurrentDatabase(before)
      end)
      say("back", (back_ok and back) and "ok" or "fail")
    else
      pcall(back_to, start)
    end
    if ok then
      say("result", result)
    else
      say("result", "failed", hex(result))
    end
    return
  end

  say("result", "failed", hex("unknown request"))
end

local ok, err = pcall(main)
if not ok then say("result", "failed", hex(err)) end
