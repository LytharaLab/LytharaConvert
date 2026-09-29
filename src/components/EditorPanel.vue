<script setup>
import { computed } from 'vue';
import {
  state, config, currentFormats, setMountedCategory, openBrowser,
  clearSelected, removeSelected, moveSelected, start, displayName, extOf, size
} from '../store.js';
import AppIcon from './AppIcon.vue';

const categories = [
  { value: 'image', label: '图片' },
  { value: 'video', label: '视频' },
  { value: 'audio', label: '音频' },
  { value: 'custom', label: '自定义' }
];

const selectedCountLabel = computed(() => (state.selected.length ? `${state.selected.length} 个文件` : '尚未选择'));
const summaryNumber = computed(() => (state.mode === 'gif' ? (state.selected.length ? 1 : 0) : state.selected.length));
const compressionEnabled = computed({
  get: () => state.options.compression || state.mode === 'compress',
  set: value => { state.options.compression = value; }
});
const compressionLocked = computed(() => state.mode === 'compress');
const qualityPercent = computed(() => Number(state.options.quality) || 78);
const qualityStyle = computed(() => ({
  background: `linear-gradient(to right,var(--accent-fill) ${qualityPercent.value}%,var(--track) ${qualityPercent.value}%)`
}));
const formatHint = computed(() => {
  if (state.category === 'custom') return '自定义扩展名交由 FFmpeg 尝试编码，是否可用取决于当前 FFmpeg 构建。';
  if (state.category === 'audio') return '视频转音频时，会提取其中的音轨。';
  return '动图或视频转静态图片时，可输出完整帧序列。';
});
const badgeText = path => (extOf(path) || '?').slice(0, 4).toUpperCase();
</script>

<template>
  <div class="left-column">
    <section class="panel input-panel">
      <div class="panel-heading">
        <div><span class="step">01</span><h2>{{ state.mode === 'gif' ? '添加 GIF 画面' : '添加文件' }}</h2></div>
        <span class="quiet">{{ selectedCountLabel }}</span>
      </div>
      <div
        class="drop-zone"
        :class="{ 'drag-over': state.dragging }"
        tabindex="0"
        role="button"
        aria-label="添加媒体文件"
        @click="openBrowser('files')"
        @keydown.enter.prevent="openBrowser('files')"
        @keydown.space.prevent="openBrowser('files')"
      >
        <div class="drop-icon"><AppIcon name="upload" /></div>
        <strong>拖入媒体文件，开始创作</strong>
        <span>或点击这里浏览本机文件 · 支持多选</span>
        <div class="drop-dots"><i></i><i></i><i></i></div>
      </div>
      <div class="file-actions">
        <button class="text-button" @click="openBrowser('files')"><AppIcon name="plus" /> 添加文件</button>
        <button class="text-button" @click="openBrowser('folder')"><AppIcon name="folder" /> 添加文件夹</button>
        <button class="text-button subtle" @click="clearSelected">清空选择</button>
      </div>
      <div class="selection-list">
        <div v-for="(file, index) in state.selected" :key="file" class="selected-file">
          <span class="file-icon">{{ badgeText(file) }}</span>
          <div class="file-info">
            <b :title="file">{{ displayName(file) }}</b>
            <small>{{ state.mode === 'gif' ? `第 ${index + 1} 帧` : file }}</small>
          </div>
          <template v-if="state.mode === 'gif'">
            <button title="上移" :disabled="index === 0" @click="moveSelected(index, -1)">↑</button>
            <button title="下移" :disabled="index === state.selected.length - 1" @click="moveSelected(index, 1)">↓</button>
          </template>
          <button title="移除文件" @click="removeSelected(index)">×</button>
        </div>
      </div>
    </section>

    <section class="panel options-panel">
      <div class="panel-heading">
        <div><span class="step">02</span><h2>输出设置</h2></div>
        <span class="quiet">细节由你掌控</span>
      </div>

      <div v-if="state.mode === 'convert'">
        <label class="field-label">输出类型</label>
        <div class="segmented">
          <button
            v-for="item in categories"
            :key="item.value"
            :class="{ chosen: state.category === item.value }"
            @click="setMountedCategory(item.value)"
          >{{ item.label }}</button>
        </div>
        <div class="form-row format-row">
          <div v-if="state.category !== 'custom'" class="field">
            <label for="format">目标格式</label>
            <select id="format" v-model="state.options.format">
              <option v-for="value in currentFormats" :key="value" :value="value">{{ value.toUpperCase() }}</option>
            </select>
          </div>
          <div v-else class="field">
            <label for="customFormat">扩展名</label>
            <input id="customFormat" v-model="state.options.customFormat" spellcheck="false" placeholder="例如 exr" maxlength="12" />
          </div>
        </div>
        <p class="hint">{{ formatHint }}</p>
      </div>

      <div v-if="state.mode === 'gif'" class="info-strip">
        <AppIcon name="film" />
        <span>按左侧图片的顺序合成 GIF。可用箭头调整帧顺序。</span>
      </div>

      <label class="optimization-toggle">
        <span><b>启用额外压缩</b><small>关闭后按高质量参数编码；有损格式本身仍会压缩</small></span>
        <input v-model="compressionEnabled" type="checkbox" :disabled="compressionLocked" />
        <i></i>
      </label>

      <div class="form-row" :style="{ opacity: compressionEnabled ? 1 : 0.45 }">
        <div class="field">
          <label for="quality">质量 <span class="value-chip">{{ qualityPercent }}</span></label>
          <input id="quality" v-model.number="state.options.quality" type="range" min="10" max="100" :style="qualityStyle" :disabled="!compressionEnabled" />
          <div class="range-caption"><span>更小体积</span><span>更高质量</span></div>
        </div>
        <div class="field">
          <label for="targetMb">目标大小 <span class="label-help">可选</span></label>
          <div class="input-unit">
            <input id="targetMb" v-model="state.options.targetMb" type="number" min="0" max="100000" step="0.1" placeholder="自动" :disabled="!compressionEnabled" />
            <span>MB</span>
          </div>
          <small class="field-help">压缩后尽量接近；无法保证精确大小</small>
        </div>
      </div>

      <div v-if="state.mode !== 'compress'" class="form-row advanced-controls">
        <div class="field">
          <label for="frameMode">转为静态图片时</label>
          <select id="frameMode" v-model="state.options.frames">
            <option value="all">提取全部帧</option>
            <option value="first">只取第一帧</option>
          </select>
        </div>
        <div class="field">
          <label for="gifFps">GIF 帧率 <span class="label-help">fps</span></label>
          <input id="gifFps" v-model.number="state.options.gifFps" type="number" min="1" max="30" />
        </div>
      </div>

      <div v-if="state.mode === 'gif'" class="form-row">
        <div class="field">
          <label for="frameSeconds">每帧持续 <span class="label-help">秒</span></label>
          <input id="frameSeconds" v-model.number="state.options.frameSeconds" type="number" min="0.05" max="10" step="0.05" />
        </div>
        <div class="field">
          <label for="maxWidth">GIF 最大宽度 <span class="label-help">像素</span></label>
          <input id="maxWidth" v-model.number="state.options.maxWidth" type="number" min="120" max="3840" />
        </div>
        <div class="field">
          <label for="singleDuration">单图循环 <span class="label-help">秒</span></label>
          <input id="singleDuration" v-model.number="state.options.singleDuration" type="number" min="1" max="120" />
        </div>
      </div>

      <div class="output-location">
        <div class="output-icon"><AppIcon name="folder" /></div>
        <div><small>保存位置</small><strong>{{ state.settings.outputDir || '读取中…' }}</strong></div>
        <button class="text-button" @click="openBrowser('output')">更改</button>
      </div>

      <div class="submit-row">
        <span><b>{{ summaryNumber }}</b> 个文件待处理</span>
        <button class="primary-button" @click="start">
          <AppIcon name="arrow" /><span>{{ config.action }}</span>
        </button>
      </div>
    </section>
  </div>
</template>
