local App = {}
App.card_title = "Parameters injected with correct /crates/ structure!"
App.button_text = "Call Luau Function"
function App.on_button_clicked(msg)
    print("[LUAU] Callback executed successfully: " .. msg)
end
return App
