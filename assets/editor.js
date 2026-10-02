"use strict";

const form = document.getElementById("editor");
const markdown = document.getElementById("markdown");
const preview = document.getElementById("preview");
const error = document.getElementById("error");
const creating = form.dataset.creating === "true";

function slug(text) {
    return text
        .normalize("NFD")
        .replace(/\p{M}/gu, "")
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "");
}

function targetUrl() {
    if (!creating) return form.dataset.path;
    const parent = form.dataset.parent === "/" ? "" : form.dataset.parent;
    return `${parent}/${document.getElementById("name").value}`;
}

function showError(message) {
    error.textContent = message;
    error.hidden = !message;
}

if (creating) {
    const title = document.getElementById("title");
    const name = document.getElementById("name");
    const address = document.getElementById("address");
    let nameEdited = false;
    const update = () => {
        address.textContent = targetUrl();
    };
    title.addEventListener("input", () => {
        if (!nameEdited) name.value = slug(title.value);
        update();
    });
    name.addEventListener("input", () => {
        nameEdited = name.value !== "";
        update();
    });
}

let mermaidLoading;
function drawDiagrams() {
    if (!preview.querySelector("pre.mermaid")) return;
    mermaidLoading ??= new Promise((resolve, reject) => {
        const script = document.createElement("script");
        script.src = "/_/static/vendor/mermaid.min.js";
        script.onload = resolve;
        script.onerror = reject;
        document.head.append(script);
    });
    mermaidLoading.then(() => {
        mermaid.initialize({
            startOnLoad: false,
            securityLevel: "strict",
            theme: matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "default",
        });
        mermaid.run({ nodes: preview.querySelectorAll("pre.mermaid") });
    });
}

async function showTab(tab) {
    for (const button of form.querySelectorAll("[data-tab]")) {
        button.setAttribute("aria-selected", String(button.dataset.tab === tab));
    }
    const previewing = tab === "preview";
    markdown.hidden = previewing;
    preview.hidden = !previewing;
    if (!previewing) return;
    preview.textContent = "Rendering…";
    const response = await fetch("/_/api/preview", { method: "POST", body: markdown.value });
    preview.innerHTML = response.ok ? await response.text() : "<p>The preview could not be rendered.</p>";
    drawDiagrams();
}

for (const button of form.querySelectorAll("[data-tab]")) {
    button.addEventListener("click", () => showTab(button.dataset.tab));
}

const dialog = document.getElementById("link-dialog");
const filter = document.getElementById("link-filter");
const list = document.getElementById("link-list");
let pages = [];
let selection = { start: 0, end: 0 };

function renderPicker() {
    const query = filter.value.toLowerCase();
    list.replaceChildren();
    for (const page of pages) {
        if (query && !page.title.toLowerCase().includes(query) && !page.url.includes(query)) continue;
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = page.title;
        const url = document.createElement("small");
        url.textContent = page.url;
        button.append(url);
        button.addEventListener("click", () => insertLink(page));
        const item = document.createElement("li");
        item.append(button);
        list.append(item);
    }
}

function insertLink(page) {
    dialog.close();
    showTab("source");
    const text = markdown.value.slice(selection.start, selection.end) || page.title;
    markdown.setRangeText(`[${text}](${page.url})`, selection.start, selection.end, "end");
    markdown.focus();
}

document.getElementById("link-button").addEventListener("click", async () => {
    selection = { start: markdown.selectionStart, end: markdown.selectionEnd };
    filter.value = "";
    list.replaceChildren();
    dialog.showModal();
    const response = await fetch("/_/api/pages");
    pages = response.ok ? await response.json() : [];
    renderPicker();
    filter.focus();
});
filter.addEventListener("input", renderPicker);

const uploadFile = document.getElementById("upload-file");
document.getElementById("upload-button").addEventListener("click", () => {
    selection = { start: markdown.selectionStart, end: markdown.selectionEnd };
    uploadFile.click();
});
uploadFile.addEventListener("change", async () => {
    const file = uploadFile.files[0];
    uploadFile.value = "";
    if (!file) return;
    showError("");
    if (file.size > 2 * 1024 * 1024) {
        showError(`${file.name} is larger than 2 MB.`);
        return;
    }
    const response = await fetch(`/_/api/uploads?name=${encodeURIComponent(file.name)}`, { method: "POST", body: file });
    if (!response.ok) {
        showError(response.status === 413 ? `${file.name} is larger than 2 MB.` : `Uploading failed (${response.status}).`);
        return;
    }
    showTab("source");
    markdown.setRangeText((await response.json()).markdown, selection.start, selection.end, "end");
    markdown.focus();
});

form.addEventListener("submit", async (event) => {
    event.preventDefault();
    showError("");
    let body = markdown.value;
    if (creating) {
        const title = document.getElementById("title").value.trim();
        if (title && !/^#\s/.test(body)) body = `# ${title}\n\n${body}`;
    }
    const url = targetUrl();
    const headers = { "Content-Type": "application/json" };
    if (creating) headers["If-None-Match"] = "*";
    const response = await fetch(`/_/api/page${url === "/" ? "" : url}`, {
        method: "PUT",
        headers,
        body: JSON.stringify({ markdown: body, protected: document.getElementById("protected").checked }),
    });
    if (response.ok) {
        location.href = (await response.json()).url;
        return;
    }
    if (response.status === 401) {
        showError("Your session has ended. Log in again in another tab, then save.");
        return;
    }
    const failure = await response.json().catch(() => ({}));
    showError(failure.error ?? `Saving failed (${response.status}).`);
});
