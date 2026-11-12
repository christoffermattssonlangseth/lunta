package app.lunta

import android.app.Application
import uniffi.lunta_core.setPdfiumPath

class LuntaApp : Application() {
    lateinit var model: AppModel
        private set

    override fun onCreate() {
        super.onCreate()
        // the PDF converter loads PDFium (libpdfium.so, packaged with the app) from here
        setPdfiumPath(applicationInfo.nativeLibraryDir + "/libpdfium.so")
        model = AppModel(this)
    }
}
