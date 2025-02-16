// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded affix "><li class="part-title">Basics</li><li class="chapter-item expanded "><a href="containers.html"><strong aria-hidden="true">1.</strong> Data Types in Slosh</a></li><li class="chapter-item expanded "><a href="equality.html"><strong aria-hidden="true">2.</strong> Equality</a></li><li class="chapter-item expanded "><a href="namespaces.html"><strong aria-hidden="true">3.</strong> Namespaces</a></li><li class="chapter-item expanded "><a href="errors.html"><strong aria-hidden="true">4.</strong> Errors</a></li><li class="chapter-item expanded "><a href="syntax-and-macros.html"><strong aria-hidden="true">5.</strong> Syntax and Macros</a></li><li class="chapter-item expanded "><a href="iterators.html"><strong aria-hidden="true">6.</strong> Iterators</a></li><li class="chapter-item expanded "><a href="shell-redirects.html"><strong aria-hidden="true">7.</strong> Shell Redirects</a></li><li class="chapter-item expanded affix "><li class="part-title">General</li><li class="chapter-item expanded "><a href="let.html"><strong aria-hidden="true">8.</strong> Let Bindings and Destructuring</a></li><li class="chapter-item expanded "><a href="namespaces.html"><strong aria-hidden="true">9.</strong> Namespaces</a></li><li class="chapter-item expanded "><a href="end_to_end.html"><strong aria-hidden="true">10.</strong> Examples</a></li><li class="chapter-item expanded affix "><li class="spacer"></li><li class="chapter-item expanded affix "><li class="part-title">Slosh Forms</li><li class="chapter-item expanded affix "><a href="src/generated-sections/all.html">All</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/char.html">Char</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/collection.html">Collection</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/conditional.html">Conditional</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/conversion.html">Conversion</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/core.html">Core</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/doc.html">Doc</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/file.html">File</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/hashmap.html">Hashmap</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/io.html">Io</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/iterator.html">Iterator</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/math.html">Math</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/pair.html">Pair</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/random.html">Random</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/scripting.html">Scripting</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/sequence.html">Sequence</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/shell.html">Shell</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/string.html">String</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/system.html">System</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/test.html">Test</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/type.html">Type</a></li><li class="chapter-item expanded affix "><a href="src/generated-sections/vector.html">Vector</a></li><li class="chapter-item expanded affix "><li class="spacer"></li><li class="chapter-item expanded affix "><li class="part-title">Supplemental Material</li><li class="chapter-item expanded affix "><a href="src/generated-sections/sl-sh -> slosh port.html">sl-sh -&gt; slosh port</a></li><li class="chapter-item expanded affix "><a href="slosh-rust-docs/doc/slosh/index.html">Slosh Rust Docs</a></li><li class="chapter-item expanded affix "><a href="all-rust-docs/doc/slosh/index.html">All Rust Docs</a></li><li class="chapter-item expanded affix "><a href="legacy/index.html">Legacy sl-sh Documentation</a></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString();
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);
