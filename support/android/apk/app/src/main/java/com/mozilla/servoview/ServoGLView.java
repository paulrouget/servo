package com.mozilla.servoview;

import android.content.Context;
import android.content.res.AssetManager;
import android.opengl.GLSurfaceView;
import android.util.AttributeSet;
import android.util.Log;
import java.io.IOException;
import java.io.InputStream;

public class ServoGLView extends GLSurfaceView {

    private static final String LOGTAG = "ServoGLView";

    private final ServoGLRenderer mRenderer;
    private final NativeServo mServo;

    public ServoGLView(Context context, AttributeSet attrs) {
        super(context, attrs);
        setFocusable(true);
        setFocusableInTouchMode(true);
        setWillNotCacheDrawing(false);
        setEGLContextClientVersion(3);
        setEGLConfigChooser(8, 8, 8, 8, 24, 0);
        mRenderer = new ServoGLRenderer(this);
        setRenderer(mRenderer);
        mServo = new NativeServo();
        Log.d(LOGTAG, "Starting Servo (" + mServo.version() + ")");
        setRenderMode(GLSurfaceView.RENDERMODE_WHEN_DIRTY);
    }

    public void onSurfaceResized(int width, int height) {
        Log.w(LOGTAG, "onViewResized()");
        queueEvent(new Runnable() {
            public void run() {
                mServo.resize(width, height);
            }
        });
    }

    class WakeupCallback implements NativeServo.WakeupCallback {
        public void wakeup(){
            Log.w(LOGTAG, "WakeupCallback::wakeup()");
            queueEvent(new Runnable() {
                public void run() {
                    mServo.performUpdates();
                }
            });
        };
    }


    class ReadFileCallback implements NativeServo.ReadFileCallback {
        public byte[] readfile(String file) {
            Log.w(LOGTAG, "ReadFileCallback::readfile(" + file + ")");
            try {
                AssetManager assetMgr = getContext().getResources().getAssets();
                InputStream stream = assetMgr.open(file);
                byte[] bytes = new byte[stream.available()];
                stream.read(bytes);
                stream.close();
                return bytes;
            } catch (IOException e) {
                // FIXME: do something
                Log.e(LOGTAG, e.getMessage());
                return null;
            }
        }
    }

    class ServoCallbacks implements NativeServo.ServoCallbacks {
        public void flush() {
            Log.w(LOGTAG, "ServoCallback::flush()");
            requestRender();
        };
        public void onLoadStarted() {
            Log.w(LOGTAG, "ServoCallback::onLoadStarted()");
            post(new Runnable() {
                public void run() {
                    // FIXME: send event or something
                }
            });
        };
        public void onLoadEnded() {
            Log.w(LOGTAG, "ServoCallback::onLoadEnded()");
            post(new Runnable() {
                public void run() {
                    // FIXME: send event or something
                }
            });
        };
        public void onTitleChanged(final String title) {
            Log.w(LOGTAG, "ServoCallback::onTitleChanged(" + title + ")");
            post(new Runnable() {
                public void run() {
                    // FIXME: send event or something
                }
            });
        };
        public void onUrlChanged(final String url) {
            Log.w(LOGTAG, "ServoCallback::onUrlChanged(" + url + ")");
            post(new Runnable() {
                public void run() {
                    // FIXME: send event or something
                }
            });
        };
        public void onHistoryChanged(final boolean canGoBack, final boolean canGoForward) {
            Log.w(LOGTAG, "ServoCallback::onHistoryChanged()");
            post(new Runnable() {
                public void run() {
                    // FIXME: send event or something
                }
            });
        };
        public void onAnimatingChanged(final boolean animating) {
            Log.w(LOGTAG, "ServoCallback::onAnimatingChanged()");
            // FIXME: use Choreagrapher
        };
    }


    public void onGLReady() {
        final WakeupCallback c1 = new WakeupCallback();
        final ReadFileCallback c2 = new ReadFileCallback();
        final ServoCallbacks c3 = new ServoCallbacks();
        final boolean showLogs = true;
        queueEvent(new Runnable() {
            public void run() {
                int width = getWidth();
                int height = getHeight();
                mServo.init("https://servo.org", c1, c2, c3, width, height, showLogs);
            }
        });

    }
}
