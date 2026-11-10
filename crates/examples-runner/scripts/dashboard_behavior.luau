local Dashboard = {}

function Dashboard.generate_metrics()
    -- Emulates checking real machinery parameters dynamically
    return {
        cpu = "CPU Load: " .. math.random(12, 88) .. "% | Temperature: 42°C",
        disk = "Storage Available: 243 GB Free / 512 GB Total",
        network = "Latency: 14ms | Status: Stable Connected",
    }
end

return Dashboard
