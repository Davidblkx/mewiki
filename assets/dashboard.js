"use strict";

async function call(button, status, request, done) {
    button.disabled = true;
    status.textContent = "Working…";
    try {
        const response = await request();
        status.textContent = response.ok ? await done(response) : `Failed (${response.status}).`;
    } catch {
        status.textContent = "Failed: the server could not be reached.";
    } finally {
        button.disabled = false;
    }
}

const saveCss = document.getElementById("save-css");
saveCss.addEventListener("click", () =>
    call(
        saveCss,
        document.getElementById("css-status"),
        () => fetch("/_/api/custom-css", { method: "PUT", body: document.getElementById("custom-css").value }),
        async () => "Saved.",
    ),
);

const rebuild = document.getElementById("rebuild");
rebuild.addEventListener("click", () =>
    call(
        rebuild,
        document.getElementById("rebuild-status"),
        () => fetch("/_/api/rebuild", { method: "POST" }),
        async (response) => `Rebuilt ${(await response.json()).pages} pages.`,
    ),
);
