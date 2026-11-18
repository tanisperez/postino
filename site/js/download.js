// The Linux package picker of the download page. Each option's value is its file's download
// address and its data-asset the file name, both filled in by docs-src/release_links.py when
// the site is published. The Download button and the file name below it follow the select.
// Without JavaScript the picker is hidden by CSS and the button downloads the AppImage.
document.querySelectorAll(".download .picker select").forEach(function (select) {
    var card = select.closest(".download");
    var button = card.querySelector(".btn");
    var file = card.querySelector("code[data-file]");

    function update() {
        var option = select.options[select.selectedIndex];
        button.href = option.value;
        if (file) {
            file.textContent = option.dataset.asset;
        }
    }

    select.addEventListener("change", update);
    update();
});
