package com.mozilla.servo;

public class NativeServo {
    private static final String LOGTAG = "java::ServoView::LibServo";
    public native String version();
    public native void init(String url,
                            WakeupCallback wakeup,
                            ReadFileCallback readfile,
                            ServoCallbacks callbacks,
                            int width, int height);
    public native void performUpdates();
    public native void resize(int width, int height);
    public native void reload();
    public native void stop();
    public native void goBack();
    public native void goForward();
    public native void loadUri(String uri);
    public native void scroll(int dx, int dy, int x, int y, int phase);
    public native void click(int x, int y);

    public NativeServo() {
        // FIXME: don't forget to strip the binary
        // FIXME: copy assets and lib post build
        // FIXME: see support/android/old_android_app/jni/Android.mk
        System.loadLibrary("c++_shared");
        System.loadLibrary("servojni");
    }

    public interface ReadFileCallback {
        byte[] readfile(String file);
    }

    public interface WakeupCallback {
        void wakeup();
    }

    public interface ServoCallbacks {
        void flush();
        void onLoadStarted();
        void onLoadEnded();
        void onTitleChanged(String title);
        void onUrlChanged(String url);
        void onHistoryChanged(boolean canGoBack, boolean canGoForward);
    }
}
