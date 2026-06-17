/**
 * Outline view component for data viewer.
 *
 * Two-pane layout: tree panel (left) + detail panel (right).
 *
 * @module data-viewer/outline
 */

import { highlightData } from "./highlighter.ts";
import { serializeData } from "./parser.ts";
import { createResizeHandle } from "../ui/resize-handle.ts";
import type { DataFormat, TreeNode } from "./types.ts";

/**
 * Outline view managing tree panel and detail panel.
 */
export class OutlineView {
  private container: HTMLElement;
  private treePanel: HTMLElement;
  private detailPanel: HTMLElement;
  private nodes: TreeNode[];
  private format: DataFormat;
  private parsedData: unknown;
  private selectedIndex = -1;
  private treeItems: HTMLElement[] = [];

  constructor(
    nodes: TreeNode[],
    format: DataFormat,
    parsedData: unknown,
  ) {
    this.nodes = nodes;
    this.format = format;
    this.parsedData = parsedData;

    this.container = document.createElement("div");
    this.container.className = "dv-outline-view";

    this.treePanel = document.createElement("div");
    this.treePanel.className = "dv-tree-panel";

    this.detailPanel = document.createElement("div");
    this.detailPanel.className = "dv-detail-panel";

    const resizeHandle = createResizeHandle(this.treePanel, {
      minWidth: 200,
      maxWidth: 600,
      storageKey: "emterm.dataViewer.treePanelWidth",
    });

    this.container.appendChild(this.treePanel);
    this.container.appendChild(resizeHandle);
    this.container.appendChild(this.detailPanel);

    this.buildTree();
    // Select root (show entire document)
    this.selectIndex(-1);
    this.showRootDetail();
  }

  getElement(): HTMLElement {
    return this.container;
  }

  private buildTree(): void {
    // Add "(root)" entry
    const rootItem = document.createElement("div");
    rootItem.className = "dv-tree-item dv-tree-item-root selected";
    rootItem.textContent = "(root)";
    rootItem.addEventListener("click", () => {
      this.selectIndex(-1);
      this.showRootDetail();
    });
    this.treePanel.appendChild(rootItem);
    this.treeItems.push(rootItem);

    for (let i = 0; i < this.nodes.length; i++) {
      const node = this.nodes[i]!;
      const item = document.createElement("div");
      item.className = "dv-tree-item";
      item.style.paddingLeft = `${(node.depth + 1) * 16 + 8}px`;

      const iconSpan = document.createElement("span");
      iconSpan.className = "dv-tree-icon";
      iconSpan.textContent = node.hasChildren ? "▸" : "";
      item.appendChild(iconSpan);
      item.appendChild(document.createTextNode(node.key));

      const idx = i;
      item.addEventListener("click", () => {
        this.selectIndex(idx);
        this.showDetail(node);
      });

      this.treePanel.appendChild(item);
      this.treeItems.push(item);
    }
  }

  private selectIndex(index: number): void {
    // Deselect previous
    const prevTreeIdx =
      this.selectedIndex === -1 ? 0 : this.selectedIndex + 1;
    if (this.treeItems[prevTreeIdx]) {
      this.treeItems[prevTreeIdx]!.classList.remove("selected");
    }
    this.selectedIndex = index;
    // Select new
    const newTreeIdx = index === -1 ? 0 : index + 1;
    if (this.treeItems[newTreeIdx]) {
      this.treeItems[newTreeIdx]!.classList.add("selected");
      this.treeItems[newTreeIdx]!.scrollIntoView({ block: "nearest" });
    }
  }

  private showRootDetail(): void {
    const text = serializeData(this.parsedData, this.format);
    this.detailPanel.innerHTML = "";
    const pre = document.createElement("pre");
    pre.className = "dv-detail-content";
    pre.innerHTML = highlightData(text, this.format);
    this.detailPanel.appendChild(pre);
  }

  private showDetail(node: TreeNode): void {
    const text = serializeData(node.value, this.format);
    this.detailPanel.innerHTML = "";
    const pre = document.createElement("pre");
    pre.className = "dv-detail-content";
    pre.innerHTML = highlightData(text, this.format);
    this.detailPanel.appendChild(pre);
  }

  /**
   * Navigate tree selection up.
   */
  navigateUp(): void {
    // -1 is root (index 0 in treeItems), nodes start at 0 (index 1 in treeItems)
    if (this.selectedIndex === -1) return; // Already at root
    const newIndex = this.selectedIndex - 1;
    this.selectIndex(newIndex);
    if (newIndex === -1) {
      this.showRootDetail();
    } else {
      this.showDetail(this.nodes[newIndex]!);
    }
  }

  /**
   * Navigate tree selection down.
   */
  navigateDown(): void {
    if (this.selectedIndex >= this.nodes.length - 1) return;
    const newIndex = this.selectedIndex + 1;
    this.selectIndex(newIndex);
    this.showDetail(this.nodes[newIndex]!);
  }

  /**
   * Navigate by a delta (positive = down, negative = up).
   * Only renders the final position, avoiding intermediate re-renders.
   */
  navigateBy(delta: number): void {
    let target = this.selectedIndex + delta;
    target = Math.max(-1, Math.min(target, this.nodes.length - 1));
    if (target === this.selectedIndex) return;
    this.selectIndex(target);
    if (target === -1) {
      this.showRootDetail();
    } else {
      this.showDetail(this.nodes[target]!);
    }
  }

  /**
   * Navigate to the first item.
   */
  navigateHome(): void {
    this.selectIndex(-1);
    this.showRootDetail();
  }

  /**
   * Navigate to the last item.
   */
  navigateEnd(): void {
    if (this.nodes.length === 0) return;
    const lastIndex = this.nodes.length - 1;
    this.selectIndex(lastIndex);
    this.showDetail(this.nodes[lastIndex]!);
  }

  dispose(): void {
    this.container.remove();
    this.treeItems = [];
  }
}
