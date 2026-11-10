-- voice_capture_behavior.luau
-- UI state controller for the voice-capture desktop app (bridged via mlua).

local M = {}

-- Returns { status_text = string, status_color = "#RRGGBB" }
function M.on_recording()
    return { status_text = "Gravando... (multimídia)", status_color = "#f7768e" }
end

function M.on_transcribing()
    return { status_text = "Transcrevendo áudio...", status_color = "#7aa2f7" }
end

function M.on_success(text)
    local preview = tostring(text or "")
    if #preview > 48 then
        preview = string.sub(preview, 1, 48) .. "…"
    end
    return {
        status_text = "Transcrição concluída",
        status_color = "#50b478",
        preview = preview,
    }
end

function M.on_error(msg)
    return { status_text = "Erro na captura/transcrição", status_color = "#f7768e", error = tostring(msg or "") }
end

function M.on_cancel()
    return { status_text = "Captura cancelada", status_color = "#f7768e" }
end

return M
