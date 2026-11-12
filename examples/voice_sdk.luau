-- voice-sdk.luau
-- Example: VoiceCaptureWindow component via the unified beca.* runtime
-- Run (GUI): cargo run -p beca -- run examples/voice_sdk.luau

-- Configure component props (beca-ui contract)
app:set("audio-path-data", "M 0 80 L 508 80")

-- Default actions (Rust) available via app:call
app:on("send", function()
  print("[LUA] send clicked")
  app:call("ui.set", "busy", "true")
  app:call("ui.set", "status-text", "Transcribing...")
  -- Example with an existing file:
  -- local path = "/tmp/opencode/beca-voice-XXXX.wav"
  -- local text = app:call("transcribe.send", path, daemon)
  -- app:set("transcript-text", text)
  -- app:call("ui.set", "show-result", "true")
  -- app:call("ui.set", "busy", "false")
  app:call("ui.set", "busy", "false")
end)

app:on("cancel", function()
  print("[LUA] cancel clicked")
  app:call("app.close")
end)

app:on("copy-text", function()
  print("[LUA] copy clicked")
  local txt = app:get("transcript-text")
  if txt and txt ~= "" then
    local ok = app:call("clipboard.copy", txt)
    print("[LUA] clipboard.copy ok=" .. tostring(ok))
  end
end)

app:on("close-app", function()
  print("[LUA] close clicked")
  app:call("app.close")
end)

-- You can also use beca.voice.transcribe directly (without a window)
-- local t = beca.voice.transcribe("/tmp/opencode/test-voice.wav", daemon)
-- print("[LUA] transcribe direct: " .. tostring(t))

print("[LUA] script loaded. Calling app:run()")
app:run()
print("[LUA] app:run() finished")
