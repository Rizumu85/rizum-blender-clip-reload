from __future__ import annotations


TRANSLATION_CONTEXT = "*"


def _entries(pairs: dict[str, str]) -> dict[tuple[str, str], str]:
    return {(TRANSLATION_CONTEXT, source): translation for source, translation in pairs.items()}


_ZH_HANS = _entries({
    "Autoreload .Clip": "自动重载 .Clip",
    "Watch every imported .clip's mtime and re-render when it changes. Rendering runs on a background thread so Blender's UI stays responsive.": "监视所有已导入 .clip 的修改时间，并在变化时重新渲染。渲染会在后台线程运行，Blender 界面保持可响应。",
    "Check Timer Frequency (s)": "检查计时频率（秒）",
    "How often to check .clip mtimes and file sizes.": "检查 .clip 修改时间和文件大小的频率。",
    "Debug log": "调试日志",
    "Print extra info to the system console.": "向系统控制台输出额外信息。",
    "Developer Mode": "开发者模式",
    "Show render timing and diagnostic actions in the image panel.": "在图像面板中显示渲染耗时和诊断操作。",
    "Packaged native renderer missing; rebuild the add-on package.": "缺少打包的 native 渲染器；请重新构建插件包。",
    "Ready": "就绪",
    "Source changed": "源文件已变化",
    "Source missing": "源文件缺失",
    "Rendering": "渲染中",
    "Render failed": "渲染失败",
    "Unknown": "未知",
    "Packed": "已打包",
    "Needs Pack": "需要打包",
    "Waiting for render": "等待渲染",
    "Packing": "打包中",
    "Pack Error": "打包错误",
    "Import Clip Studio (.clip)": "导入 Clip Studio (.clip)",
    "Manual Reload": "手动重载",
    "Re-render the .clip file this image was imported from.": "重新渲染此图像来源的 .clip 文件。",
    "Pack Clip Studio Image": "打包 Clip Studio 图像",
    "Pack current pixels now. Saving the .blend also packs images that need it.": "立即打包当前像素。保存 .blend 时也会打包需要打包的图像。",
    "Pack current pixels into the .blend now. Saving the .blend also packs Needs Pack images automatically.": "立即把当前像素打包进 .blend。保存 .blend 时也会自动打包“需要打包”的图像。",
    "Toggle Support Details": "切换支持详情",
    "Copy Support Diagnostics": "复制支持诊断",
    "Copy Layer Locations": "复制图层位置",
    "Open Clip Studio Diagnostics": "打开 Clip Studio 诊断",
    "Source: {name}": "来源：{name}",
    "Pack": "打包",
    "Pack error: {message}": "打包错误：{message}",
    "Unsupported native nodes: {count}": "不支持的 native 节点：{count}",
    "Show fewer unsupported details": "收起不支持详情",
    "Show all unsupported details": "显示全部不支持详情",
    "{count} more unsupported item(s)": "还有 {count} 个不支持项",
    "Copy layer locations": "复制图层位置",
    "Packed pixels are still visible.": "已打包的像素仍然可见。",
    "Error: {message}": "错误：{message}",
    "Elapsed: {seconds}": "已用时间：{seconds}",
    "Last render: {seconds}": "上次渲染：{seconds}",
    "Open Diagnostics": "打开诊断",
    "Copy Diagnostic": "复制诊断",
    "Rendering in background": "正在后台渲染",
    "Already rendering {name}": "已在渲染 {name}",
    "Rendering {name} in the background": "正在后台渲染 {name}",
    "Reloading {name} in the background": "正在后台重载 {name}",
    "Clip Studio render failed": "Clip Studio 渲染失败",
    "Failed to render {name}: {message}": "渲染 {name} 失败：{message}",
    "Source .clip not found: {path}": "找不到源 .clip：{path}",
    "Wait for the current render before packing": "请等待当前渲染完成后再打包",
    "Pack failed: {message}": "打包失败：{message}",
    "Packed {name} in {seconds}": "已打包 {name}，耗时 {seconds}",
    "No unsupported layer locations": "没有不支持的图层位置",
    "Copied Clip Studio diagnostics": "已复制 Clip Studio 诊断",
    "Copied Clip Studio layer locations": "已复制 Clip Studio 图层位置",
    "Opened {name}": "已打开 {name}",
    "Wrote {name}": "已写入 {name}",
})


_JA_JP = _entries({
    "Autoreload .Clip": ".Clip を自動リロード",
    "Watch every imported .clip's mtime and re-render when it changes. Rendering runs on a background thread so Blender's UI stays responsive.": "読み込んだ .clip の更新時刻を監視し、変更時に再レンダーします。レンダーはバックグラウンドスレッドで実行されるため、Blender の UI は応答したままです。",
    "Check Timer Frequency (s)": "チェック間隔（秒）",
    "How often to check .clip mtimes and file sizes.": ".clip の更新時刻とファイルサイズを確認する頻度です。",
    "Debug log": "デバッグログ",
    "Print extra info to the system console.": "追加情報をシステムコンソールへ出力します。",
    "Developer Mode": "開発者モード",
    "Show render timing and diagnostic actions in the image panel.": "画像パネルにレンダー時間と診断操作を表示します。",
    "Packaged native renderer missing; rebuild the add-on package.": "同梱 native レンダラーが見つかりません。アドオンパッケージを再ビルドしてください。",
    "Ready": "準備完了",
    "Source changed": "ソースが変更されました",
    "Source missing": "ソースが見つかりません",
    "Rendering": "レンダー中",
    "Render failed": "レンダー失敗",
    "Unknown": "不明",
    "Packed": "パック済み",
    "Needs Pack": "パックが必要",
    "Waiting for render": "レンダー待機中",
    "Packing": "パック中",
    "Pack Error": "パックエラー",
    "Import Clip Studio (.clip)": "Clip Studio (.clip) を読み込み",
    "Manual Reload": "手動リロード",
    "Re-render the .clip file this image was imported from.": "この画像の読み込み元 .clip を再レンダーします。",
    "Pack Clip Studio Image": "Clip Studio 画像をパック",
    "Pack current pixels now. Saving the .blend also packs images that need it.": "現在のピクセルを今すぐパックします。.blend 保存時にも必要な画像をパックします。",
    "Pack current pixels into the .blend now. Saving the .blend also packs Needs Pack images automatically.": "現在のピクセルを .blend に今すぐパックします。.blend 保存時にも「パックが必要」な画像を自動でパックします。",
    "Toggle Support Details": "対応詳細を切り替え",
    "Copy Support Diagnostics": "対応診断をコピー",
    "Copy Layer Locations": "レイヤー位置をコピー",
    "Open Clip Studio Diagnostics": "Clip Studio 診断を開く",
    "Source: {name}": "ソース：{name}",
    "Pack": "パック",
    "Pack error: {message}": "パックエラー：{message}",
    "Unsupported native nodes: {count}": "未対応 native ノード：{count}",
    "Show fewer unsupported details": "未対応詳細を少なく表示",
    "Show all unsupported details": "未対応詳細をすべて表示",
    "{count} more unsupported item(s)": "未対応項目がさらに {count} 件",
    "Copy layer locations": "レイヤー位置をコピー",
    "Packed pixels are still visible.": "パック済みピクセルは表示されたままです。",
    "Error: {message}": "エラー：{message}",
    "Elapsed: {seconds}": "経過：{seconds}",
    "Last render: {seconds}": "前回レンダー：{seconds}",
    "Open Diagnostics": "診断を開く",
    "Copy Diagnostic": "診断をコピー",
    "Rendering in background": "バックグラウンドでレンダー中",
    "Already rendering {name}": "{name} はすでにレンダー中です",
    "Rendering {name} in the background": "{name} をバックグラウンドでレンダー中",
    "Reloading {name} in the background": "{name} をバックグラウンドでリロード中",
    "Clip Studio render failed": "Clip Studio のレンダーに失敗しました",
    "Failed to render {name}: {message}": "{name} のレンダーに失敗しました：{message}",
    "Source .clip not found: {path}": "ソース .clip が見つかりません：{path}",
    "Wait for the current render before packing": "パックする前に現在のレンダー完了を待ってください",
    "Pack failed: {message}": "パック失敗：{message}",
    "Packed {name} in {seconds}": "{name} を {seconds} でパックしました",
    "No unsupported layer locations": "未対応レイヤー位置はありません",
    "Copied Clip Studio diagnostics": "Clip Studio 診断をコピーしました",
    "Copied Clip Studio layer locations": "Clip Studio レイヤー位置をコピーしました",
    "Opened {name}": "{name} を開きました",
    "Wrote {name}": "{name} を書き込みました",
})


_ES = _entries({
    "Autoreload .Clip": "Recargar .Clip automáticamente",
    "Watch every imported .clip's mtime and re-render when it changes. Rendering runs on a background thread so Blender's UI stays responsive.": "Vigila la fecha de modificación de cada .clip importado y vuelve a renderizar cuando cambia. El render se ejecuta en segundo plano para mantener la interfaz de Blender activa.",
    "Check Timer Frequency (s)": "Frecuencia de comprobación (s)",
    "How often to check .clip mtimes and file sizes.": "Cada cuánto comprobar la fecha de modificación y el tamaño de los .clip.",
    "Debug log": "Registro de depuración",
    "Print extra info to the system console.": "Muestra información adicional en la consola del sistema.",
    "Developer Mode": "Modo desarrollador",
    "Show render timing and diagnostic actions in the image panel.": "Muestra tiempos de render y acciones de diagnóstico en el panel de imagen.",
    "Packaged native renderer missing; rebuild the add-on package.": "Falta el renderizador native empaquetado; reconstruye el paquete del complemento.",
    "Ready": "Listo",
    "Source changed": "Fuente cambiada",
    "Source missing": "Falta la fuente",
    "Rendering": "Renderizando",
    "Render failed": "Error de render",
    "Unknown": "Desconocido",
    "Packed": "Empaquetado",
    "Needs Pack": "Necesita empaquetar",
    "Waiting for render": "Esperando render",
    "Packing": "Empaquetando",
    "Pack Error": "Error al empaquetar",
    "Import Clip Studio (.clip)": "Importar Clip Studio (.clip)",
    "Manual Reload": "Recarga manual",
    "Re-render the .clip file this image was imported from.": "Vuelve a renderizar el archivo .clip del que se importó esta imagen.",
    "Pack Clip Studio Image": "Empaquetar imagen Clip Studio",
    "Pack current pixels now. Saving the .blend also packs images that need it.": "Empaqueta los píxeles actuales ahora. Al guardar el .blend también se empaquetan las imágenes que lo necesiten.",
    "Pack current pixels into the .blend now. Saving the .blend also packs Needs Pack images automatically.": "Empaqueta los píxeles actuales en el .blend ahora. Al guardar el .blend también se empaquetan automáticamente las imágenes que lo necesiten.",
    "Toggle Support Details": "Alternar detalles de soporte",
    "Copy Support Diagnostics": "Copiar diagnóstico de soporte",
    "Copy Layer Locations": "Copiar ubicaciones de capas",
    "Open Clip Studio Diagnostics": "Abrir diagnóstico de Clip Studio",
    "Source: {name}": "Fuente: {name}",
    "Pack": "Empaquetar",
    "Pack error: {message}": "Error al empaquetar: {message}",
    "Unsupported native nodes: {count}": "Nodos native no compatibles: {count}",
    "Show fewer unsupported details": "Mostrar menos detalles no compatibles",
    "Show all unsupported details": "Mostrar todos los detalles no compatibles",
    "{count} more unsupported item(s)": "{count} elemento(s) no compatible(s) más",
    "Copy layer locations": "Copiar ubicaciones de capas",
    "Packed pixels are still visible.": "Los píxeles empaquetados siguen visibles.",
    "Error: {message}": "Error: {message}",
    "Elapsed: {seconds}": "Transcurrido: {seconds}",
    "Last render: {seconds}": "Último render: {seconds}",
    "Open Diagnostics": "Abrir diagnóstico",
    "Copy Diagnostic": "Copiar diagnóstico",
    "Rendering in background": "Renderizando en segundo plano",
    "Already rendering {name}": "{name} ya se está renderizando",
    "Rendering {name} in the background": "Renderizando {name} en segundo plano",
    "Reloading {name} in the background": "Recargando {name} en segundo plano",
    "Clip Studio render failed": "Falló el render de Clip Studio",
    "Failed to render {name}: {message}": "No se pudo renderizar {name}: {message}",
    "Source .clip not found: {path}": "No se encontró el .clip fuente: {path}",
    "Wait for the current render before packing": "Espera a que termine el render actual antes de empaquetar",
    "Pack failed: {message}": "Error al empaquetar: {message}",
    "Packed {name} in {seconds}": "{name} empaquetado en {seconds}",
    "No unsupported layer locations": "No hay ubicaciones de capas no compatibles",
    "Copied Clip Studio diagnostics": "Diagnóstico de Clip Studio copiado",
    "Copied Clip Studio layer locations": "Ubicaciones de capas de Clip Studio copiadas",
    "Opened {name}": "{name} abierto",
    "Wrote {name}": "{name} escrito",
})


TRANSLATIONS = {
    "zh_HANS": _ZH_HANS,
    "zh_CN": _ZH_HANS,
    "ja_JP": _JA_JP,
    "es": _ES,
    "es_ES": _ES,
}


def _language_code(bpy_module) -> str:
    try:
        language = bpy_module.context.preferences.view.language
    except Exception:
        return ""
    return str(language or "")


def _entries_for_language(language: str) -> dict[tuple[str, str], str] | None:
    if language in TRANSLATIONS:
        return TRANSLATIONS[language]
    if language.startswith("zh"):
        return TRANSLATIONS["zh_HANS"]
    if language.startswith("ja"):
        return TRANSLATIONS["ja_JP"]
    if language.startswith("es"):
        return TRANSLATIONS["es"]
    return None


def translate(bpy_module, message: str) -> str:
    entries = _entries_for_language(_language_code(bpy_module))
    if not entries:
        return message
    return entries.get((TRANSLATION_CONTEXT, message), message)


def register(bpy_module, addon_package: str) -> None:
    try:
        bpy_module.app.translations.unregister(addon_package)
    except Exception:
        pass
    bpy_module.app.translations.register(addon_package, TRANSLATIONS)


def unregister(bpy_module, addon_package: str) -> None:
    try:
        bpy_module.app.translations.unregister(addon_package)
    except Exception:
        pass
