-- voice-sdk.luau
-- Exemplo: componente VoiceCaptureWindow via runtime unificado beca.*
-- Rode (GUI): cargo run -p beca -- run voice-sdk.luau
-- ou com bin antigo: cargo run -p desktop-app --bin sdk-run -- voice-sdk.luau

local daemon = (beca.voice.config().daemon_url) or "http://127.0.0.1:4100"
local app = beca.ui.window("VoiceCaptureWindow")

-- Configurar props do componente (contrato da ui-lib)
app:set("status-text", "Pronto para capturar")
app:set("audio-path-data", "M 0 80 L 508 80")

-- Ações padrão (Rust) acessíveis via app:call
app:on("send", function()
  print("[LUA] send clicked")
  app:call("ui.set", "busy", "true")
  app:call("ui.set", "status-text", "Transcrevendo áudio...")
  -- Exemplo com arquivo existente:
  -- local path = "/tmp/opencode/voice-capture-XXXX.wav"
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

-- Também é possível usar beca.voice.transcribe diretamente (sem janela)
-- local t = beca.voice.transcribe("/tmp/opencode/test-voice.wav", daemon)
-- print("[LUA] transcribe direct: " .. tostring(t))

print("[LUA] script carregado. Chamando app:run()")
app:run()
print("[LUA] app:run() finalizado")
