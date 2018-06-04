package com.mozilla.servoview;

import android.content.Context;
import android.content.res.AssetManager;
import android.net.Uri;
import android.opengl.GLSurfaceView;
import android.util.AttributeSet;
import android.util.Log;
import android.view.Choreographer;
import android.view.GestureDetector;
import android.view.MotionEvent;
import android.widget.OverScroller;

import java.io.IOException;
import java.io.InputStream;

public class ServoView extends GLSurfaceView implements GestureDetector.OnGestureListener, Choreographer.FrameCallback {

    private static final String LOGTAG = "ServoView";

    private NativeServo mServo;
    private Client mClient = null;
    private Uri mInitialUri = Uri.parse("about:blank");

    public ServoView(Context context, AttributeSet attrs) {
        super(context, attrs);
        setFocusable(true);
        setFocusableInTouchMode(true);
        setWillNotCacheDrawing(false);
        setEGLContextClientVersion(3);
        setEGLConfigChooser(8, 8, 8, 8, 24, 0);
        ServoGLRenderer mRenderer = new ServoGLRenderer(this);
        setRenderer(mRenderer);
        mServo = new NativeServo();
        setRenderMode(GLSurfaceView.RENDERMODE_WHEN_DIRTY);
        initGestures(context);
    }

    public void reload() {
        queueEvent(() -> mServo.reload());
    }

    public void goBack() {
        queueEvent(() -> mServo.goBack());
    }

    public void goForward() {
        queueEvent(() -> mServo.goForward());
    }

    public void stop() {
        queueEvent(() -> mServo.stop());
    }

    public void onSurfaceResized(int width, int height) {
        queueEvent(() -> mServo.resize(width, height));
    }

    public void loadUri(Uri uri) {
        if (mServo != null) {
            queueEvent(() -> mServo.loadUri(uri.toString()));
        } else {
            mInitialUri = uri;
        }
    }

    class WakeupCallback implements NativeServo.WakeupCallback {
        public void wakeup() {
            queueEvent(() -> mServo.performUpdates());
        };
    }

    class ReadFileCallback implements NativeServo.ReadFileCallback {
        public byte[] readfile(String file) {
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
            requestRender();
        }

        public void onLoadStarted() {
            if (mClient != null) {
                post(() -> mClient.onLoadStarted());
            }
        }

        public void onLoadEnded() {
            if (mClient != null) {
                post(() -> mClient.onLoadEnded());
            }
        }

        public void onTitleChanged(final String title) {
            if (mClient != null) {
                post(() -> mClient.onTitleChanged(title));
            }
        }

        public void onUrlChanged(final String url) {
            if (mClient != null) {
                post(() -> mClient.onUrlChanged(url));
            }
        }

        public void onHistoryChanged(final boolean canGoBack, final boolean canGoForward) {
            if (mClient != null) {
                post(() -> mClient.onHistoryChanged(canGoBack, canGoForward));
            }
        }

        public void onAnimatingChanged(final boolean animating) {
            // FIXME: use Choreographer
        }
    }

    public void onGLReady() {
        final WakeupCallback c1 = new WakeupCallback();
        final ReadFileCallback c2 = new ReadFileCallback();
        final ServoCallbacks c3 = new ServoCallbacks();
        final boolean showLogs = true;
        queueEvent(() -> {
            int width = getWidth();
            int height = getHeight();
            mServo.init(mInitialUri.toString(), c1, c2, c3, width, height, showLogs);
        });
    }

    public interface Client {
        void onLoadStarted();
        void onLoadEnded();
        void onTitleChanged(String title);
        void onUrlChanged(String url);
        void onHistoryChanged(boolean canGoBack, boolean canGoForward);
    }

    public void setClient(Client client) {
        mClient = client;
    }

    // Scroll and click

    private GestureDetector mGestureDetector;
    private OverScroller mScroller;
    private int mLastY = 0;
    private int mCurY = 0;
    private int mLastX = 0;
    private int mCurX = 0;
    private boolean mFlinging;

    private void initGestures(Context context) {
        mGestureDetector = new GestureDetector(context, this);
        mScroller = new OverScroller(context);
    }

    @Override
    public void doFrame(long frameTimeNanos) {
        if (mScroller.isFinished() && mFlinging) {
            mFlinging = false;
            queueEvent(() -> {
                mServo.scroll(0, 0, 0, 0, 2);
                mServo.performUpdates();
            });
            return;
        }

        if (mFlinging) {
            mScroller.computeScrollOffset();
            mCurY = mScroller.getCurrY();
            mCurX = mScroller.getCurrX();
        }

        int dy = mCurY - mLastY;
        int dx = mCurX - mLastX;

        mLastY = mCurY;
        mLastX = mCurX;

        if (dy != 0 || dx != 0) queueEvent(() -> {
            // FIXME: don't use 0,0 as coordinates
            mServo.scroll(dx, dy, 0, 0, 1);
            mServo.performUpdates();
        });

        Choreographer.getInstance().postFrameCallback(this);
    }

    public boolean onFling(MotionEvent e1, MotionEvent e2, float velocityX, float velocityY) {
        int mPageWidth = 80000;
        int mPageHeight = 80000;
        // FIXME: boundaries
        // https://github.com/servo/servo/issues/20361
        mFlinging = true;
        // FIXME: magic value
        mCurY = 40000;
        mLastY = mCurY;
        mCurX = 40000;
        mLastX = mCurX;

        mScroller.fling(mCurX, mCurY, (int)velocityX, (int)velocityY, 0, mPageWidth, 0, mPageHeight);
        return true;
    }

    public boolean onDown(MotionEvent e) {
        mScroller.forceFinished(true);
        return true;
    }

    public boolean onTouchEvent(final MotionEvent e) {
        mGestureDetector.onTouchEvent(e);

        int action = e.getActionMasked();
        switch(action) {
            case (MotionEvent.ACTION_DOWN):
                mCurY = (int)e.getY();
                mLastY = mCurY;
                mCurX = (int)e.getX();
                mLastX = mCurX;
                mScroller.forceFinished(true);
                queueEvent(() -> {
                    mServo.scroll(0, 0, 0, 0, 0);
                    mServo.performUpdates();
                });
                Choreographer.getInstance().postFrameCallback(this);
                return true;
            case (MotionEvent.ACTION_MOVE):
                mCurY = (int)e.getY();
                mCurX = (int)e.getY();
                return true;
            case (MotionEvent.ACTION_UP):
            case (MotionEvent.ACTION_CANCEL):
                if (!mFlinging) {
                    queueEvent(() -> {
                        mServo.scroll(0, 0, 0, 0, 2);
                        mServo.performUpdates();
                    });
                    Choreographer.getInstance().removeFrameCallback(this);
                }
                return true;
            default:
                return true;
        }
    }

    public boolean onSingleTapUp(MotionEvent e) {
        queueEvent(() -> mServo.click((int)e.getX(), (int)e.getY()));
        return false;
    }

    public void onLongPress(MotionEvent e) { }
    public boolean onScroll(MotionEvent e1, MotionEvent e2, float distanceX, float distanceY) { return true; }
    public void onShowPress(MotionEvent e) { }

}
