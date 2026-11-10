local Wizard = {
    current_step = 1,
    steps = {
        { title = "Step 1: License Agreement", desc = "Please read and accept the terms of service to continue." },
        { title = "Step 2: Choose Location", desc = "Select the target folder where you want application files to be installed." },
        { title = "Step 3: Ready to Install", desc = "Click continue to begin the process. This might take a few moments." },
    },
}

-- Method to update properties based on internal state
function Wizard.get_current_view()
    local data = Wizard.steps[Wizard.current_step]
    return {
        title = data.title,
        desc = data.desc,
        is_first = Wizard.current_step == 1,
    }
end

-- Handlers called from the Rust bridge
function Wizard.on_next()
    if Wizard.current_step < #Wizard.steps then
        Wizard.current_step = Wizard.current_step + 1
        print("[LUAU] Advanced to step " .. Wizard.current_step)
    else
        print("[LUAU] Setup sequence complete!")
    end
    return Wizard.get_current_view()
end

function Wizard.on_back()
    if Wizard.current_step > 1 then
        Wizard.current_step = Wizard.current_step - 1
        print("[LUAU] Regressed to step " .. Wizard.current_step)
    end
    return Wizard.get_current_view()
end

return Wizard
