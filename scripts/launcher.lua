-- beca launcher (módulo launcher)
-- Uso: beca mod launcher:show (comportamento embutido) ou via runtime de scripts

local search = beca.ui.search_window({
    title = "beca launcher",
    placeholder = "Search applications..."
})

search.on_search = function(text)
    search:set_results(beca.apps.search(text or ""))
end

search.on_select = function(name)
    local ok, err = pcall(beca.apps.launch, name)
    if not ok then
        print("launch error: " .. tostring(err))
    end
end

-- Populate initial results so the list is not empty on open.
search:set_results(beca.apps.search(""))
search:show()
