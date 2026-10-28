// Copy buttons on the command blocks of the download page. Shell prompts are not part of the
// command. Without the Clipboard API (an insecure context, an old browser) no button is added.
(function () {
    if (!navigator.clipboard) {
        return;
    }
    document.querySelectorAll(".channel .code").forEach(function (block) {
        var pre = block.querySelector("pre");
        if (!pre) {
            return;
        }
        var button = document.createElement("button");
        button.type = "button";
        button.className = "copy-code";
        button.textContent = "Copy";
        button.setAttribute("aria-label", "Copy the commands");
        button.addEventListener("click", function () {
            var clone = pre.cloneNode(true);
            clone.querySelectorAll(".prompt").forEach(function (prompt) {
                prompt.remove();
            });
            navigator.clipboard.writeText(clone.textContent).then(function () {
                button.textContent = "Copied";
                setTimeout(function () {
                    button.textContent = "Copy";
                }, 1500);
            });
        });
        block.appendChild(button);
    });
})();
