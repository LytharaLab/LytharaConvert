<script setup>
import { computed } from 'vue';
import { state, browseTo, jumpTo, togglePick, confirmBrowser, closeBrowser, size } from '../store.js';
import AppIcon from './AppIcon.vue';

const TITLES = {
  files: '选择媒体文件',
  folder: '选择输入文件夹',
  output: '选择输出文件夹'
};

const title = computed(() => TITLES[state.browser.purpose] || '选择文件');
const pickable = computed(() => state.browser.purpose === 'files');
const confirmLabel = computed(() =>
  pickable.value ? `加入选中的 ${state.browser.picked.length} 个文件` : '选择此文件夹'
);
const picked = path => state.browser.picked.includes(path);

function activate(entry) {
  if (entry.isDir) browseTo(entry.path);
  else if (pickable.value) togglePick(entry);
}

function changeRoot(event) {
  const value = event.target.value;
  if (value) browseTo(value);
}
</script>

<template>
  <div v-if="state.browser.open" class="modal-overlay" role="dialog" aria-modal="true" :aria-label="title" @click.self="closeBrowser">
    <div class="modal browser-modal">
      <div class="modal-header">
        <h2>{{ title }}</h2>
        <button class="icon-button" aria-label="关闭" @click="closeBrowser">×</button>
      </div>

      <div class="browser-bar">
        <select :value="''" @change="changeRoot">
          <option value="">磁盘…</option>
          <option v-for="root in state.browser.roots" :key="root" :value="root">{{ root }}</option>
        </select>
        <button class="text-button" :disabled="!state.browser.parent" @click="browseTo(state.browser.parent)">
          <AppIcon name="up" /> 上一级
        </button>
        <input
          class="browser-input"
          :value="state.browser.draft"
          spellcheck="false"
          placeholder="粘贴绝对路径后回车，例如 D:\videos\2026"
          @input="state.browser.draft = $event.target.value"
          @keydown.enter="jumpTo($event.target.value)"
        />
        <button class="text-button" @click="jumpTo(state.browser.draft)">转到</button>
      </div>

      <div class="browser-list">
        <p v-if="state.browser.loading" class="browser-note">读取中…</p>
        <p v-else-if="state.browser.error" class="browser-note error">{{ state.browser.error }}</p>
        <p v-else-if="!state.browser.entries.length" class="browser-note">这个文件夹是空的</p>
        <button
          v-for="entry in state.browser.entries"
          :key="entry.path"
          class="browser-row"
          :class="{ picked: picked(entry.path), dim: !entry.isDir && !pickable }"
          @click="activate(entry)"
        >
          <span class="browser-mark">
            <AppIcon v-if="entry.isDir" name="folder" />
            <AppIcon v-else-if="picked(entry.path)" name="check" />
            <span v-else class="browser-dot"></span>
          </span>
          <span class="browser-name">{{ entry.name }}</span>
          <span class="browser-size">{{ entry.isDir ? '' : size(entry.size) }}</span>
        </button>
      </div>

      <p class="browser-hint">
        {{ pickable ? '地址栏可直接粘贴文件夹或文件绝对路径后回车；点文件夹进入，点文件勾选（可多选）。' : '地址栏可直接粘贴绝对路径后回车；进入目标文件夹后点右下角确认。' }}
      </p>

      <div class="browser-actions">
        <button class="text-button subtle" @click="closeBrowser">取消</button>
        <button class="primary-button" :disabled="pickable && !state.browser.picked.length" @click="confirmBrowser">
          {{ confirmLabel }}
        </button>
      </div>
    </div>
  </div>
</template>
