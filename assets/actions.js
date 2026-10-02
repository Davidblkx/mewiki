"use strict";

const actions = document.getElementById("page-actions");
const path = actions.dataset.path;
const moveTo = document.getElementById("move-to");
const confirmBox = document.getElementById("confirm");
const actionsError = document.getElementById("actions-error");
const buttons = [document.getElementById("move"), document.getElementById("delete")];

function showActionsError(message) {
    actionsError.textContent = message;
    actionsError.hidden = !message;
}

function ask(text, pages, yesLabel) {
    document.getElementById("confirm-text").textContent = text;
    const list = document.getElementById("confirm-list");
    list.replaceChildren(
        ...pages.map((page) => {
            const item = document.createElement("li");
            item.textContent = page;
            return item;
        }),
    );
    const yes = document.getElementById("confirm-yes");
    const no = document.getElementById("confirm-no");
    yes.textContent = yesLabel;
    confirmBox.hidden = false;
    for (const button of buttons) button.disabled = true;
    yes.focus();
    return new Promise((resolve) => {
        const finish = (answer) => {
            confirmBox.hidden = true;
            for (const button of buttons) button.disabled = false;
            yes.onclick = null;
            no.onclick = null;
            resolve(answer);
        };
        yes.onclick = () => finish(true);
        no.onclick = () => finish(false);
    });
}

async function failure(response) {
    if (response.status === 401) return "Your session has ended. Log in again in another tab, then retry.";
    const body = await response.json().catch(() => ({}));
    return body.error ?? `It failed (${response.status}).`;
}

async function move(makePublic) {
    const response = await fetch("/_/api/move", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ from: path, to: moveTo.value.trim(), make_public: makePublic }),
    });
    if (response.ok) {
        location.href = (await response.json()).url;
        return;
    }
    const body = await response.clone().json().catch(() => ({}));
    if (body.confirm === "make_public") {
        if (await ask("Moving it there makes these pages public:", body.pages, "Move and make public")) {
            await move(true);
        }
        return;
    }
    showActionsError(await failure(response));
}

document.getElementById("move").addEventListener("click", () => {
    showActionsError("");
    move(false);
});

document.getElementById("delete").addEventListener("click", async () => {
    showActionsError("");
    const listing = await fetch("/_/api/pages");
    const pages = listing.ok ? (await listing.json()).map((page) => page.url) : [];
    const going = [path, ...pages.filter((url) => url.startsWith(`${path}/`))];
    const text =
        going.length === 1
            ? "Delete this page? It can't be undone."
            : `Delete this page and its ${going.length - 1} subpages? It can't be undone.`;
    if (!(await ask(text, going, "Delete"))) return;
    const response = await fetch(`/_/api/page${path}?subpages=true`, { method: "DELETE" });
    if (response.ok) {
        location.href = path.slice(0, path.lastIndexOf("/")) || "/";
        return;
    }
    showActionsError(await failure(response));
});
