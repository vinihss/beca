local M = {}

function M.on_idle()
    return { status_text = "Ready to record", status_color = "#7aa2f7" }
end

function M.on_recording()
    return { status_text = "Recording...", status_color = "#f7768e" }
end

function M.on_transcribing()
    return { status_text = "Transcribing...", status_color = "#7aa2f7" }
end

function M.on_responding()
    return { status_text = "Thinking...", status_color = "#e0af68" }
end

function M.on_speaking()
    return { status_text = "Speaking...", status_color = "#bb9af7" }
end

function M.on_success(text)
    local preview = tostring(text or "")
    if #preview > 48 then
        preview = string.sub(preview, 1, 48) .. "…"
    end
    return {
        status_text = "Transcription finished",
        status_color = "#50b478",
        preview = preview,
    }
end

function M.on_error(msg)
    return { status_text = "Capture/transcription failed", status_color = "#f7768e", error = tostring(msg or "") }
end

function M.on_cancel()
    return { status_text = "Capture cancelled", status_color = "#f7768e" }
end

return M
