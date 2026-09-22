import AppKit
import WebKit

struct Job: Decodable { let html: String; let output: String; let width: Double; let height: Double; let full_resolution: Bool?; let output_width: Int?; let output_height: Int? }
final class Renderer: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
    var view: WKWebView!
    var window: NSWindow!
    var job: Job?
    var generation = 0
    var timeout: DispatchWorkItem?
    func start() {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.userContentController.add(self, name: "studio")
        config.userContentController.addUserScript(WKUserScript(source: "window.ipc={postMessage:s=>window.webkit.messageHandlers.studio.postMessage(s)};window.addEventListener(\"error\",e=>window.ipc.postMessage(JSON.stringify({type:\"render-error\",error:e.message})));", injectionTime: .atDocumentStart, forMainFrameOnly: true))
        view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = self
        window = NSWindow(contentRect:NSRect(x:-10000,y:-10000,width:1440,height:960),styleMask:[.borderless],backing:.buffered,defer:false)
        window.contentView = view
        window.orderBack(nil)
        next()
    }
    func next() {
        DispatchQueue.global().async {
            guard let line = readLine() else { DispatchQueue.main.async { NSApp.terminate(nil) }; return }
            do {
                let next = try JSONDecoder().decode(Job.self,from:Data(line.utf8))
                DispatchQueue.main.async { self.render(next) }
            } catch { DispatchQueue.main.async { self.finish(error:"Invalid thumbnail job") } }
        }
    }
    func render(_ next: Job) {
        generation += 1
        job = next
        let size = NSSize(width:min(3840,max(240,next.width)),height:min(4096,max(240,next.height)))
        window.setContentSize(size)
        view.frame = NSRect(origin:.zero,size:size)
        let current = generation
        let work = DispatchWorkItem { if self.generation == current { self.finish(error:"Thumbnail rendering timed out") } }
        timeout = work
        DispatchQueue.main.asyncAfter(deadline:.now()+10,execute:work)
        view.loadHTMLString(next.html,baseURL:nil)
    }
    func userContentController(_ userContentController: WKUserContentController,didReceive message:WKScriptMessage) {
        guard message.frameInfo.isMainFrame, let raw=message.body as? String,
              let data=raw.data(using:.utf8), let payload=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any],
              let type=payload["type"] as? String else { return }
        if type == "render-error" { finish(error:payload["error"] as? String ?? "Renderer script failed");return }
        guard type == "thumbnail-ready", let active=job else { return }
        let current=generation
        let config=WKSnapshotConfiguration()
        config.snapshotWidth=NSNumber(value:active.output_width.map(Double.init) ?? (active.full_resolution == true ? active.width : 480))
        view.takeSnapshot(with:config) { image,error in
            guard current == self.generation else { return }
            guard let image=image else { self.finish(error:"Could not capture screen");return }
            let bitmap: NSBitmapImageRep?
            if active.full_resolution == true || active.output_width != nil {
                // WK snapshots inherit the display's backing scale. Normalize exports
                // to exactly one output pixel per authored CSS pixel.
                bitmap=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:active.output_width ?? Int(active.width),pixelsHigh:active.output_height ?? Int(active.height),bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)
                if let bitmap=bitmap,let context=NSGraphicsContext(bitmapImageRep:bitmap) {
                    NSGraphicsContext.saveGraphicsState()
                    NSGraphicsContext.current=context
                    image.draw(in:NSRect(x:0,y:0,width:Double(active.output_width ?? Int(active.width)),height:Double(active.output_height ?? Int(active.height))),from:.zero,operation:.copy,fraction:1)
                    NSGraphicsContext.restoreGraphicsState()
                } else { self.finish(error:"Could not allocate export image");return }
            } else {
                bitmap=image.tiffRepresentation.flatMap { NSBitmapImageRep(data:$0) }
            }
            guard let png=bitmap?.representation(using:.png,properties:[:]) else { self.finish(error:"Could not encode screen");return }
            do {
                let url=URL(fileURLWithPath:active.output)
                try FileManager.default.createDirectory(at:url.deletingLastPathComponent(),withIntermediateDirectories:true)
                try png.write(to:url,options:.atomic)
                self.finish(error:nil)
            } catch { self.finish(error:"Could not save thumbnail") }
        }
    }
    func webView(_ webView:WKWebView,decidePolicyFor navigationAction:WKNavigationAction,decisionHandler:@escaping(WKNavigationActionPolicy)->Void) {
        let url=navigationAction.request.url?.absoluteString ?? ""
        decisionHandler(url == "about:blank" || url == "about:srcdoc" ? .allow : .cancel)
    }
    func finish(error:String?) {
        timeout?.cancel();timeout=nil;generation += 1
        let response:[String:Any] = ["output":job?.output ?? "","error":error as Any? ?? NSNull()]
        if let data=try? JSONSerialization.data(withJSONObject:response),let line=String(data:data,encoding:.utf8){print(line);fflush(stdout)}
        job=nil;view?.stopLoading();next()
    }
}
let app=NSApplication.shared
app.setActivationPolicy(.prohibited)
let renderer=Renderer()
renderer.start()
app.run()
