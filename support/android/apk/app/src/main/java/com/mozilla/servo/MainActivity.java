package com.mozilla.servo;

import android.app.Activity;
import android.os.Bundle;
import android.view.View;
import android.widget.Button;
import android.widget.EditText;
import android.widget.ProgressBar;

import com.mozilla.servoview.ServoView;

public class MainActivity extends Activity implements ServoView.Client {

    ServoView mServoView;
    Button mBackButton;
    Button mFwdButton;
    Button mReloadButton;
    Button mStopButton;
    EditText mUrlField;
    ProgressBar mProgressBar;


    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);

        mServoView = (ServoView)findViewById(R.id.servoview);
        mBackButton = (Button)findViewById(R.id.backbutton);
        mFwdButton = (Button)findViewById(R.id.forwardbutton);
        mReloadButton = (Button)findViewById(R.id.reloadbutton);
        mStopButton = (Button)findViewById(R.id.stopbutton);
        mUrlField = (EditText)findViewById(R.id.urlfield);
        mProgressBar = (ProgressBar)findViewById(R.id.progressbar);

        mServoView.setClient(this);
        mBackButton.setEnabled(false);
        mFwdButton.setEnabled(false);
        mProgressBar.setProgress(0);
    }

    public void onReloadClicked(View v) {
        mServoView.reload();
    }

    @Override
    public void onLoadStarted() {
        mReloadButton.setEnabled(false);
        mStopButton.setEnabled(true);
        mReloadButton.setVisibility(View.GONE);
        mStopButton.setVisibility(View.VISIBLE);
        mProgressBar.setVisibility(View.VISIBLE);
    }

    @Override
    public void onLoadEnded() {
        mReloadButton.setEnabled(true);
        mStopButton.setEnabled(false);
        mReloadButton.setVisibility(View.VISIBLE);
        mStopButton.setVisibility(View.GONE);
        mProgressBar.setVisibility(View.INVISIBLE);
    }

    @Override
    public void onTitleChanged(String title) {

    }

    @Override
    public void onUrlChanged(String url) {
        mUrlField.setText(url);
    }

    @Override
    public void onHistoryChanged(boolean canGoBack, boolean canGoForward) {
        mBackButton.setEnabled(canGoBack);
        mFwdButton.setEnabled(canGoForward);
    }

}
